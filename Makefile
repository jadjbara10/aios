# AIOS v0.1-alpha build orchestration
# Author: Jad Jbara <jad@aios.dev>
KERNEL    := crates/kernel
BOOTLOADER := crates/bootloader
KERNEL_TARGET := targets/x86_64-aios.json
UEFI_TARGET  := x86_64-unknown-uefi
QEMU      := qemu-system-x86_64
BUILD     := build

.PHONY: all kernel bootloader boot-image run fmt lint clean

all: boot-image

kernel:
	cargo build --target $(KERNEL_TARGET) -p aios-kernel
	cp target/$(KERNEL_TARGET)/debug/aios $(BUILD)/aios-kernel.elf

bootloader: kernel
	cargo build --target $(UEFI_TARGET) -p aios-bootloader
	cp target/$(UEFI_TARGET)/debug/aios-boot.efi $(BUILD)/BOOTX64.EFI

boot-image: bootloader
	@# Build a FAT32 image containing /EFI/BOOT/BOOTX64.EFI for OVMF.
	dd if=/dev/zero of=$(BUILD)/aios-boot.img bs=1M count=32
	mformat -i $(BUILD)/aios-boot.img -F ::
	mmd -i $(BUILD)/aios-boot.img ::/EFI ::/EFI/BOOT
	mcopy -i $(BUILD)/aios-boot.img $(BUILD)/BOOTX64.EFI ::/EFI/BOOT/

run: boot-image
	$(QEMU) -drive if=pflash,format=raw,readonly=on,file=tools/OVMF.fd \
	       -drive format=raw,file=$(BUILD)/aios-boot.img \
	       -serial stdio -m 256M

fmt:
	cargo fmt --all

lint:
	cargo fmt --all -- --check
	cargo clippy --target $(KERNEL_TARGET) -- -D warnings
	cargo clippy --target $(UEFI_TARGET) -- -D warnings

clean:
	cargo clean
	rm -rf $(BUILD)
