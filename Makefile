SHELL=/bin/bash

UID := $(shell id -u)

all: prepare
	cargo run

prepare: mount
	fusermount -u hdd-mnt

mount: | hdd.img
	fuse-ext2 hdd.img hdd-mnt -o rw+ -o allow_other -o uid=$(UID)

hdd.img:
	dd if=/dev/null of=hdd.img bs=1M seek=200
	mkfs.ext2 -F hdd.img