#![no_std]
#![feature(asm)]
#![feature(global_asm)]

//由于使用到宏，需要进行设置
//同时，这个module还必须放在其他module前
#[macro_use]
mod io;

mod init;
mod interrupt;
mod lang_items;
mod sbi;
