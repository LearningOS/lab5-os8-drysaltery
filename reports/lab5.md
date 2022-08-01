```
简单总结你实现的功能（200字以内，不要贴代码）及你完成本次实验所用的时间。

完成问答题。

(optional) 你对本次实验设计及难度/工作量的看法，以及有哪些需要改进的地方，欢迎畅所欲言。
```


# lab5实验报告
## 实现功能

按照算法说明实现了死锁检测，分别在取锁和信号量P操作处添加了检测代码。为了实现在ProcessControlBlockInner中添加了检测开启与关闭标识、锁与线程的分配矩阵与需求矩阵、信号量与线程的分配矩阵与需求矩阵，并在创建线程、创建锁、创建信号量时更新它们。

## 简答作业

1. 在我们的多线程实现中，当主线程 (即 0 号线程) 退出时，视为整个进程退出， 此时需要结束该进程管理的所有线程并回收其资源。 - 需要回收的资源有哪些？ - 其他线程的 TaskControlBlock 可能在哪些位置被引用，分别是否需要回收，为什么？

    __答__：地址空间、文件、pid。在进程的tasks中，是weak引用，不需要回收。TaskManager中等待调度的，需要回收。等待互斥使用资源的，需要回收。
    
2. 对比以下两种 Mutex.unlock 的实现，二者有什么区别？这些区别可能会导致什么问题？

```rust
impl Mutex for Mutex1 {
    fn unlock(&self) {
        let mut mutex_inner = self.inner.exclusive_access();
        assert!(mutex_inner.locked);
        mutex_inner.locked = false;
        if let Some(waking_task) = mutex_inner.wait_queue.pop_front() {
            add_task(waking_task);
        }
    }
}

impl Mutex for Mutex2 {
    fn unlock(&self) {
        let mut mutex_inner = self.inner.exclusive_access();
        assert!(mutex_inner.locked);
        if let Some(waking_task) = mutex_inner.wait_queue.pop_front() {
            add_task(waking_task);
        } else {
            mutex_inner.locked = false;
        }
    }
}
```

__答__：Mutex2在已经没有等待队列的情况下将锁的标志设置为false，让别的线程可以获取锁。而Mutex1却没有设置，可能导致后续线程无法获取锁。
    
    
