## 1. 独立可执行程序

1. 去掉std标准库依赖
    ```rust
    // src/main.rs
    #![no_std]
    ```

2. 移除std后要处理几个依赖std的
    * panic_handler
        ```rust
        // src/main.rs
        use core::panic::PanicInfo;

        #[panic_handler]
        fn panic(_: &PanicInfo)->!{
            loop{}
        }
        ```

    * eh_personality
        ```rust
        // Cargo.toml
        [profile.dev]
        panic = "abort"
        [profile.release]
        panic = "abort"
        ```

3. 重新C runtime
    1. 不用常规入口点
        ```rust
        // src/main.rs
        #![no_main]
        ```
    
    2. 覆盖C runtime入口
        ```rust
        // src/main.rs
        #[no_mangle]
        pub extern "C" fn _start() -> !{
            loop{}
        }
        ```
    3. 编译时禁用常规的C启动例程
        ```
        将 cargo build 替换成以下命令
        $ cargo rustc -- -C link-arg=-nostartfiles
        ```

---
## 2. 最小化内核

1. 查看Rust编辑器内置64位RISCV架构开发内核
    ```rust
    $ rustc -Z unstable-options --print target-spec-json --target riscv64imac-unknown-none-elf
    
    由于输出的json里面有 "panic-strategy":"abort" , 所以将Cargo.toml设置删除
    // Cargo.toml
    -[profile.dev]
    -panic = "abort"
    -[profile.release]
    -panic = "abort"
    ```
2. 使用riscv64的目标编译项目
    * 安装 riscv64imac-unknown-none-elf
    ```
    $ rustup target add riscv64imac-unknown-none-elf
    ```
    
    * 编译
    ```
    cargo build --target riscv64imac-unknown-none-elf
    ```

3. 设置默认target
    * 在项目根目录下创建 .cargo/config
    ```
    #  .cargo/config
    [build]
    target = "riscv64imac-unknown-none-elf"
    ```

4. 生成最小的内核
    * 安装工具集 objdump (查看程序信息)、 objcopy (生成文件)
    ```
    $ cargo install cargo-binutils
    $ rustup component add llvm-tools-preview
    ```

    * rust-objdump (查看程序信息)
    ```
    # -x 选项：查看程序元信息
    rust-objdump target/riscv64imac-unknown-none-elf/debug/os -x --arch-name=riscv64

    # -d 选项：查看反汇编
    rust-objdump target/riscv64imac-unknown-none-elf/debug/os -d --arch-name=riscv64
    ```

    * rust-objcopy (生成文件)  从elf格式文件生成kernel.bin
    ```
    # --strip-all 参数：丢弃符号表和调试信息
    # -O binary 参数：输出二进制文件

    $ rust-objcopy target/riscv64imac-unknown-none-elf/debug/os \
        --strip-all -O binary \
        target/riscv64imac-unknown-none-elf/debug/kernel.bin
    ```

---
###  指定内存布局

* 默认普通用户程序数据放在低地址，从查看程序元信息可以看到，生成的二进制默认是低地址开始
* 但os内核从高地址开始，RISCV物理地址具体从0x80000000开始

1. 编写linker script
``` 
// src/boot/linker64.ld

OUTPUT_ARCH(riscv)
ENTRY(_start)

BASE_ADDRESS = 0X80200000;

SECTIONS{
    . = BASE_ADDRESS;
    start = .;

    .text : {
        stext = .;
        *(.text.entry)
        *(.text .text.*)
        . = ALIGN(4K);
        etext = .;
    }

    .rodata : {
        srodata = .;
        *(.rodata .rodata.*)
        . = ALIGN(4K);
        erodata = .;
    }

    .data : {
        sdata = .;
        *(.data .data.*)
        edata = .;
    }

    .stack : {
        *(.bss.stack)
    }

    .bss : {
        sbss = .;
        *(.bss .bss.*)
        ebss = .;
    }

    PROVIDE(end = .);
}
```

2. 使用linker script
```
// .cargo/config

[target.riscv64imac-unknown-none-elf]
rustflags = ["-C", "link-arg=-Tsrc/boot/linker64.ld"]
```

3. 验证一下
* cargo build。再用 rust-objdump 查看一下

---
###  用汇编重写程序入口_start
* 将C runtime的入口点_start，改写为 ***设置内核环境变量*** 的地方
    1. 删除src/main.rs 里面的fn _start()->!
    2. 理解如何设置环境变量
        > 在cpu加电或reset后，它首先会进行**自检**，再进入**bootloader**。
        >> 在bootloader中，首先进行搜索外设，再将内核从硬盘读到内存中，并开始执行内核bin

    3. riscv提供了bootloader，就是 **OpenSBI** firmware(固件)
        > *在x86中，BIOS/UEFI是一种 firmware，在RISCV中，OpenSBI是一种 firmware*  
        * OpenSBI运行在M Mode，将要实现的OS运行在S Mode，普遍用户程序运行在U Mode
        * OpenSBI所做的一件事，就是把cpu从M Mode切换到S Mode，然后跳转到一个固定的地址(0x80200000)，开始执行代码
    4. 用汇编编写_start
    ```
    # src/boot/entry64.asm 
    
        .section .text.entry
        .globl _start

    _start:
        la sp, bootstacktop
        call rust_main



        .section .bss.stack
        .align 12
        .global bootstack

    bootstack:



        .space 4096 * 4
        .global bootstacktop

    bootstacktop:
    ``` 

    5. 在`src/main.rs`中调用 C runtime 的汇编 `_start` ，并添加rust_main函数，对应着_start的call
    ```rust
    // src/main.rs

    #![feature(global_asm)] //添加宏
    #![feature(asm)] 

    global_asm!(include_str!("boot/entry64.asm")); // 使用宏加载汇编


    pub fn console_putchar(ch: u8){
        let ret: usize;
        let arg0: usize = ch as usize;
        let arg1: usize = 0;
        let arg2: usize = 0;
        let which: usize = 1;
        unsafe {
            asm!("ecall"
                : "={x10}" (ret)
                : "{x10}" (arg0), "{x11}" (arg1), "{x12}" (arg2), "{x17}" (which)
                : "memory"
                : "volatile"
            );
        }
    }

    #[no_mangle]
    pub extern "C" fn rust_main() -! {
        // 在屏幕上输出"OK\n", 随后进入死循环
        console_putchar(b'O');
        console_putchar(b'K');
        console_putchar(b'\n');
        loop{}
    }
    ```

---
###  linux下载Qemu
```
# 下载
$ wget https://download.qemu.org/qemu-4.1.1.tar.xz
$ tar xvJf qemu-4.1.1.tar.xz

# 编译支持RISCV的qemu
$ cd qemu-4.1.1
$ ./configure --target-list=riscv32-softmmu,riscv64-softmmu
$ make -j
$ export PATH=$PWD/riscv32-softmmu:$PWD/riscv64-softmmu:$PATH

# 在每次开机之后使用以下命令来允许模拟器使用内存（不是必须的），否则无法正常使用qemu
$ sudo sysctl vm.overcommit_memory=1

# 确认qemu
$ qemu-system-riscv64 --version
```
* 新版的qemu内置OpenSBI，运行qemu，使用 Ctrl+a+x 退出。
```
$ qemu-system-riscv64 \
  --machine virt \
  --nographic \
  --bios default
```

* 使用Makefile自动构建内核并使用qemu运行
```makefile
# Makefile

target := riscv64imac-unknown-none-elf
mode := debug
kernel := target/$(target)/$(mode)/os
bin := target/$(target)/$(mode)/kernel.bin

objdump := rust-objdump --arch-name=riscv64
objcopy := rust-objcopy --binary-architecture=riscv64

.PHONY: kernel build clean qemu run env

env:
    cargo install cargo-binutils
    rustup component add llvm-tools-preview rustfmt
    rustup target add $(target)

kernel:
    cargo build

$(bin): kernel
    $(objcopy) $(kernel) --strip-all -O binary $@

asm:
    $(objdump) -d $(kernel) | less

build: $(bin)

clean:
    cargo clean

qemu: build
    qemu-system-riscv64 \
        -machine virt \
        -nographic \
        -bios default \
        -device loader,file=$(bin),addr=0x80200000

run: build qemu
```

---
## 3. 中断
* 中断的分类
    * 异常(Exception)
        > 最常见的异常包括：  
        访问无效内存地址  
        执行非法指令,如除零(不可恢复)  
        发生缺页(可以恢复)
    * 陷入(Trap)
        > 指主动通过一条命令停下来，并跳转到处理函数  
        常见有：通过`ecall`进行 系统调用(syscall)  
        通过`ebreak`进入 断点(breakpoint)
    * 外部中断(Interrupt)
        > 指外设发来的信号，cpu必须停下处理该信号。这种中断是异步的。  
        典型有：定时器倒计时结束、串口收到数据
 
* 中断相关的寄存器  
    * sepc(exception program counter)
        > 记录触发中断的指令地址
    * scause
        > 记录中断原因
    * stval
        > 记录中断的辅助信息  
        例如：取指、访存、缺页异常，stval会记录目标地址    
    * stvec
        > * 设置如何寻找S Mode中断程序的起始地址  
        > * 保存 中断向量表 基址 BASE
        > * 设置MODE
        >> 当MODE=0, 为Direct模式，无论什么中断，都直接跳转到基址`pc <- BASE`  
        当MODE=1, 为Vectored模式，遇到中断就`pc <- BASE + 4 x cause`。
        将中断处理程序放在正确的位置，设置好stvec，遇到中断硬件会根据中断原因跳转相应的处理程序
    *  sstatus
        > S Mode 的 控制状态寄存器。保存全局的 中断使能标志 和其他状态。  
        可以设置sstatus来中断使能与否
    
* 中断相关的指令
    * ecall(environment call)
        >当在S Mode调用`ecall`，触发ecall-from-s-mode-exception  
        会从S Mode进入M Mode的中断处理流程（如设置定时器）  
        
        >当在M Mode调用`ecall`，触发ecall-from-u-mode-exception  
        会从M Mode进入S Mode的中断处理流程（如执行系统调用）  
    * ebreak(environment break)
        > 触发一个断点中断，进入中断处理流程
    * sret
        > 用于S Mode返回U Mode，实际作用`pc <- sepc`，返回中断前的位置
    * mret
        > 用于M Mode返回S/U Mode，实际作用`pc <- mepc`，返回中断前的位置
---
### 手动触发断点中断
1. 在os初始化时，设置中断处理程序的起始地址，并使能中断
    1. 引入对寄存器操作的库
    ```rust
    // Cargo.toml
    [dependencies]
    riscv = {git="https://github.com/rcore-os/riscv", features=["inline-asm"]} 
    ```
    2. 设置中断处理程序起始地址
    ```
    为了方便起见，我们先将stvec设置为Direct模式，统一跳转到一个处理程序
    
    // src/lib.rs
    mod interrupt;
    
    // src/interrupt.rs
    use riscv::register::{
        scause,
        sepc,
        stvec,
        sscratch
    };
    
    pub fn init(){
        unsafe {
            sscratch::write(0);
            stvec::write(trap_handler as usize, stvec::TrapMode::Direct);
        }
        println!("++++ setup interrupt! ++++"); 
    }
    
    fn trap_handler () -> {
        let cause = scause::read().cause();
        let epc = sepc::read();
        println!("trap: cause: {:?}, epc: 0x{:#x}", cause, epc);
        panic!("trap handled!");
    }
    ```
