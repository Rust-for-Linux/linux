#!/bin/sh

busybox insmod rust_minimal.ko
busybox  rmmod rust_minimal.ko

busybox insmod rust_print.ko
busybox  rmmod rust_print.ko

busybox insmod rust_module_parameters.ko
busybox  rmmod rust_module_parameters.ko

busybox insmod rust_sync.ko
busybox  rmmod rust_sync.ko

busybox insmod rust_chrdev.ko
busybox  rmmod rust_chrdev.ko

busybox insmod rust_miscdev.ko
busybox  rmmod rust_miscdev.ko

busybox insmod rust_stack_probing.ko
busybox  rmmod rust_stack_probing.ko

busybox insmod rust_semaphore.ko
busybox  rmmod rust_semaphore.ko

busybox insmod rust_semaphore_c.ko
busybox  rmmod rust_semaphore_c.ko

busybox insmod rust_module_parameters_loadable_default.ko
busybox insmod rust_module_parameters_loadable_custom.ko \
    my_bool=n \
    my_i32=345543 \
    my_str=🦀mod \
    my_usize=84 \
    my_array=1,2,3
busybox  rmmod rust_module_parameters_loadable_default.ko
busybox  rmmod rust_module_parameters_loadable_custom.ko

busybox insmod rust_seq_file.ko
busybox mkdir proc
busybox mount -t proc proc /proc
busybox mkdir debugfs
busybox mount -t debugfs debugfs /debugfs
export RUST_SEQ_MINOR=$(busybox cat /proc/misc | busybox grep rust_seq_file | busybox cut -d ' ' -f 1)
busybox mknod /dev/rust_seq_file0 c 10 $RUST_SEQ_MINOR
busybox cat /dev/rust_seq_file0
busybox cat /dev/rust_seq_file0
busybox cat /debugfs/rust_seq_file
busybox rm /dev/rust_seq_file0
busybox rmmod rust_seq_file.ko

busybox reboot -f
