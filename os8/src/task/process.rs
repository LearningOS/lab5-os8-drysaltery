use super::id::RecycleAllocator;
use super::{add_task, pid_alloc, PidHandle, TaskControlBlock};
use crate::fs::{File, Stdin, Stdout};
use crate::mm::{translated_refmut, MemorySet, KERNEL_SPACE};
use crate::sync::{Condvar, Mutex, Semaphore, UPSafeCell};
use crate::trap::{trap_handler, TrapContext};
use alloc::string::String;
use alloc::sync::{Arc, Weak};
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefMut;

pub struct ProcessControlBlock {
    // immutable
    pub pid: PidHandle,
    // mutable
    inner: UPSafeCell<ProcessControlBlockInner>,
}

// LAB5 HINT: you may add data structures for deadlock detection here
pub struct ProcessControlBlockInner {
    pub is_zombie: bool,
    pub memory_set: MemorySet,
    pub parent: Option<Weak<ProcessControlBlock>>,
    pub children: Vec<Arc<ProcessControlBlock>>,
    pub exit_code: i32,
    pub fd_table: Vec<Option<Arc<dyn File + Send + Sync>>>,
    pub tasks: Vec<Option<Arc<TaskControlBlock>>>,
    pub task_res_allocator: RecycleAllocator,
    pub mutex_list: Vec<Option<Arc<dyn Mutex>>>,
    pub semaphore_list: Vec<Option<Arc<Semaphore>>>,
    pub condvar_list: Vec<Option<Arc<Condvar>>>,
    pub enable_check: bool,     // 能否允许进行死锁检测
    pub mutex_avail: [usize; 20],
    pub sema_avail: [usize; 20],
    pub mutex_alloc: [[usize; 20]; 20],
    pub sema_alloc: [[usize; 20]; 20],
    pub sema_need: [[usize; 20]; 20],
}

impl ProcessControlBlockInner {
    #[allow(unused)]
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }

    pub fn alloc_fd(&mut self) -> usize {
        if let Some(fd) = (0..self.fd_table.len()).find(|fd| self.fd_table[*fd].is_none()) {
            fd
        } else {
            self.fd_table.push(None);
            self.fd_table.len() - 1
        }
    }

    pub fn alloc_tid(&mut self) -> usize {
        self.task_res_allocator.alloc()
    }

    pub fn dealloc_tid(&mut self, tid: usize) {
        self.task_res_allocator.dealloc(tid)
    }

    pub fn thread_count(&self) -> usize {
        self.tasks.len()
    }

    pub fn get_task(&self, tid: usize) -> Arc<TaskControlBlock> {
        self.tasks[tid].as_ref().unwrap().clone()
    }

    pub fn deadlock_test_mutex(&self, mutex_id: usize, tid: usize) -> bool {
        // 1. 初始化
        // 向量 work 初始化为当前进程可用资源实例数向量
        let task_len = self.tasks.len();
        let mut work: [usize; 20] = self.mutex_avail;
        // 向量 Finish 每个分量都为 false，表示初态都是未完成
        let mut finish: Vec<bool> = vec![false; task_len];
        let mut need: Vec<[usize; 20]> = vec![[0usize; 20]; task_len];
        need[tid][mutex_id] = 1;
        //2. 
        let mut change: bool = true;	// 循环控制变量.
        while change {
            //Request[i]表示矩阵的第i行,也就是进程Pi所申请的资
            //源数向量,若进程Pi未完成,并且需求能够得到满足,就执
            //行Pi,后续释放其中的资源,等价于Work获得Pi已经持有
            //的资源数向量Allocation[i].
            change = false;
            for i in 0..task_len {
                if finish[i] == false && work[mutex_id] >= need[i][mutex_id] {
                    work[mutex_id] += self.mutex_alloc[i][mutex_id];
                    finish[i] = true;
                    change = true;
                }
            }
        }
        // 只有当 finish 和 mutex_alloc 不再发生改变时,
        // 也就意味着所有的进程都已经完成或者剩余进程都无法运行
        // 这两种情况之一发生时，才会跳出while循环.

        //3.
        for i in 0..task_len {
            //只要存在一个进程是false,就代表出现了死锁
            //返回真值表示检测到了死锁
            if finish[i] == false {
                return true;
            }
        }
        return false;
    }

    pub fn deadlock_test_sema(&self, sema_id: usize, tid: usize) -> bool {
        // 1. 初始化
        // 向量 work 初始化为当前进程可用资源实例数向量
        let task_len = self.tasks.len();
        let mut work: [usize; 20] = self.sema_avail;
        // 向量 Finish 每个分量都为 false，表示初态都是未完成
        let mut finish: Vec<bool> = vec![false; task_len];
        let mut need = self.sema_need;
        need[tid][sema_id] += 1;
        //2. 
        let mut change: bool = true;	// 循环控制变量.
        while change {
            //Request[i]表示矩阵的第i行,也就是进程Pi所申请的资
            //源数向量,若进程Pi未完成,并且需求能够得到满足,就执
            //行Pi,后续释放其中的资源,等价于Work获得Pi已经持有
            //的资源数向量Allocation[i].
            change = false;
            for i in 0..task_len {
                let mut flag = true;
                for j in 0..20 {
                    if work[j] < need[i][j] {
                        flag = false;
                        break;
                    }
                }
                if finish[i] == false && flag {
                    for j in 0..20 {
                        work[j] += self.sema_alloc[i][j];
                    }
                    // work[sema_id] += self.sema_alloc[i][sema_id];
                    finish[i] = true;
                    change = true;
                }
            }
        }
        // 只有当 finish 和 mutex_alloc 不再发生改变时,
        // 也就意味着所有的进程都已经完成或者剩余进程都无法运行
        // 这两种情况之一发生时，才会跳出while循环.

        //3.
        for i in 0..task_len {
            //只要存在一个进程是false,就代表出现了死锁
            //返回真值表示检测到了死锁
            if finish[i] == false {
                return true;
            }
        }
        return false;
    }

}

impl ProcessControlBlock {
    pub fn inner_exclusive_access(&self) -> RefMut<'_, ProcessControlBlockInner> {
        self.inner.exclusive_access()
    }

    // LAB5 HINT: How to initialize deadlock data structures?
    pub fn new(elf_data: &[u8]) -> Arc<Self> {
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, ustack_base, entry_point) = MemorySet::from_elf(elf_data);
        // allocate a pid
        let pid_handle = pid_alloc();
        let process = Arc::new(Self {
            pid: pid_handle,
            inner: unsafe {
                UPSafeCell::new(ProcessControlBlockInner {
                    is_zombie: false,
                    memory_set,
                    parent: None,
                    children: Vec::new(),
                    exit_code: 0,
                    fd_table: vec![
                        // 0 -> stdin
                        Some(Arc::new(Stdin)),
                        // 1 -> stdout
                        Some(Arc::new(Stdout)),
                        // 2 -> stderr
                        Some(Arc::new(Stdout)),
                    ],
                    tasks: Vec::new(),
                    task_res_allocator: RecycleAllocator::new(),
                    mutex_list: Vec::new(),
                    semaphore_list: Vec::new(),
                    condvar_list: Vec::new(),
                    enable_check: false,
                    mutex_avail: [0usize; 20],
                    sema_avail: [0usize; 20],
                    mutex_alloc: [[0usize; 20]; 20],
                    sema_alloc: [[0usize; 20]; 20],
                    sema_need: [[0usize; 20]; 20],
                })
            },
        });
        // create a main thread, we should allocate ustack and trap_cx here
        let task = Arc::new(TaskControlBlock::new(
            Arc::clone(&process),
            ustack_base,
            true,
        ));
        // prepare trap_cx of main thread
        let task_inner = task.inner_exclusive_access();
        let trap_cx = task_inner.get_trap_cx();
        let ustack_top = task_inner.res.as_ref().unwrap().ustack_top();
        let kernel_stack_top = task.kernel_stack.get_top();
        drop(task_inner);
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            ustack_top,
            KERNEL_SPACE.exclusive_access().token(),
            kernel_stack_top,
            trap_handler as usize,
        );
        // add main thread to the process
        let mut process_inner = process.inner_exclusive_access();
        process_inner.tasks.push(Some(Arc::clone(&task)));
        drop(process_inner);
        // add main thread to scheduler
        add_task(task);
        process
    }

    // LAB5 HINT: How to initialize deadlock data structures?
    /// Load a new elf to replace the original application address space and start execution
    /// Only support processes with a single thread.
    pub fn exec(self: &Arc<Self>, elf_data: &[u8], args: Vec<String>) {
        assert_eq!(self.inner_exclusive_access().thread_count(), 1);
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, ustack_base, entry_point) = MemorySet::from_elf(elf_data);
        let new_token = memory_set.token();
        // substitute memory_set
        self.inner_exclusive_access().memory_set = memory_set;
        // then we alloc user resource for main thread again
        // since memory_set has been changed
        let task = self.inner_exclusive_access().get_task(0);
        let mut task_inner = task.inner_exclusive_access();
        task_inner.res.as_mut().unwrap().ustack_base = ustack_base;
        task_inner.res.as_mut().unwrap().alloc_user_res();
        task_inner.trap_cx_ppn = task_inner.res.as_mut().unwrap().trap_cx_ppn();
        // push arguments on user stack
        let mut user_sp = task_inner.res.as_mut().unwrap().ustack_top();
        user_sp -= (args.len() + 1) * core::mem::size_of::<usize>();
        let argv_base = user_sp;
        let mut argv: Vec<_> = (0..=args.len())
            .map(|arg| {
                translated_refmut(
                    new_token,
                    (argv_base + arg * core::mem::size_of::<usize>()) as *mut usize,
                )
            })
            .collect();
        *argv[args.len()] = 0;
        for i in 0..args.len() {
            user_sp -= args[i].len() + 1;
            *argv[i] = user_sp;
            let mut p = user_sp;
            for c in args[i].as_bytes() {
                *translated_refmut(new_token, p as *mut u8) = *c;
                p += 1;
            }
            *translated_refmut(new_token, p as *mut u8) = 0;
        }
        // make the user_sp aligned to 8B for k210 platform
        user_sp -= user_sp % core::mem::size_of::<usize>();
        // initialize trap_cx
        let mut trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            task.kernel_stack.get_top(),
            trap_handler as usize,
        );
        trap_cx.x[10] = args.len();
        trap_cx.x[11] = argv_base;
        *task_inner.get_trap_cx() = trap_cx;
    }

    // LAB5 HINT: How to initialize deadlock data structures?
    /// Fork from parent to child
    /// Only support processes with a single thread.
    pub fn fork(self: &Arc<Self>) -> Arc<Self> {
        let mut parent = self.inner_exclusive_access();
        assert_eq!(parent.thread_count(), 1);
        // clone parent's memory_set completely including trampoline/ustacks/trap_cxs
        let memory_set = MemorySet::from_existed_user(&parent.memory_set);
        // alloc a pid
        let pid = pid_alloc();
        // copy fd table
        let mut new_fd_table: Vec<Option<Arc<dyn File + Send + Sync>>> = Vec::new();
        for fd in parent.fd_table.iter() {
            if let Some(file) = fd {
                new_fd_table.push(Some(file.clone()));
            } else {
                new_fd_table.push(None);
            }
        }
        // create child process pcb
        let child = Arc::new(Self {
            pid,
            inner: unsafe {
                UPSafeCell::new(ProcessControlBlockInner {
                    is_zombie: false,
                    memory_set,
                    parent: Some(Arc::downgrade(self)),
                    children: Vec::new(),
                    exit_code: 0,
                    fd_table: new_fd_table,
                    tasks: Vec::new(),
                    task_res_allocator: RecycleAllocator::new(),
                    mutex_list: Vec::new(),
                    semaphore_list: Vec::new(),
                    condvar_list: Vec::new(),
                    enable_check: false,
                    mutex_avail: [0usize; 20],
                    sema_avail: [0usize; 20],
                    mutex_alloc: [[0usize; 20]; 20],
                    sema_alloc: [[0usize; 20]; 20],
                    sema_need: [[0usize; 20]; 20],
                })
            },
        });
        // add child
        parent.children.push(Arc::clone(&child));
        // create main thread of child process
        let task = Arc::new(TaskControlBlock::new(
            Arc::clone(&child),
            parent
                .get_task(0)
                .inner_exclusive_access()
                .res
                .as_ref()
                .unwrap()
                .ustack_base(),
            // here we do not allocate trap_cx or ustack again
            // but mention that we allocate a new kernel_stack here
            false,
        ));
        // attach task to child process
        let mut child_inner = child.inner_exclusive_access();
        child_inner.tasks.push(Some(Arc::clone(&task)));
        drop(child_inner);
        // modify kernel_stack_top in trap_cx of this thread
        let task_inner = task.inner_exclusive_access();
        let trap_cx = task_inner.get_trap_cx();
        trap_cx.kernel_sp = task.kernel_stack.get_top();
        drop(task_inner);
        // add this thread to scheduler
        add_task(task);
        child
    }

    pub fn getpid(&self) -> usize {
        self.pid.0
    }

    pub fn kernel_process() -> Arc<Self> {
        let memory_set = MemorySet::kernel_copy();
        let process = Arc::new(ProcessControlBlock {
            pid: super::pid_alloc(),
            inner: unsafe {
                UPSafeCell::new(ProcessControlBlockInner {
                    is_zombie: false,
                    memory_set: memory_set,
                    parent: None,
                    children: Vec::new(),
                    exit_code: 0,
                    fd_table: Vec::new(),
                    tasks: Vec::new(),
                    task_res_allocator: RecycleAllocator::new(),
                    mutex_list: Vec::new(),
                    semaphore_list: Vec::new(),
                    condvar_list: Vec::new(),
                    enable_check: false,
                    mutex_avail: [0usize; 20],
                    sema_avail: [0usize; 20],
                    mutex_alloc: [[0usize; 20]; 20],
                    sema_alloc: [[0usize; 20]; 20],
                    sema_need: [[0usize; 20]; 20],
                })
            },
        });
        process
    }

}
