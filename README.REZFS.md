# How to setup RezFS

1. ```cp RUSTFS_CONFIG .config```
2. ```make LLVM=1```
3. ```sudo make modules_install && sudo make install```
4. ```sudo reboot```
5. ```Select your newly built kernel in GRUB```
6. Create a disk image: ```dd bs=4096 count=1000 if=/dev/zero of=ez_disk.img```
7. Format the disk image: ```fs/rustezfs/format_disk_as_ezfs ez_disk.img 1000```
8. Find a loop device: ```sudo losetup --find --show ez_disk.img```
9. Load file system: ```sudo insmod fs/rustezfs/rustezfs.ko```
10. Create mount point: ```mkdir /mnt/ez```
11. Mount file system: ```sudo mount -t rustezfs /dev/loop<N> /mnt/ez```

if you land in trouble you can follow this guide for kernel development: https://w4118.github.io/
