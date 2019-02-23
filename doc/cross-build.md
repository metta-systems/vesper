# Canadian-Cross-build rustc for arvm7 RPi3 to build for aarch64-unknown

Japaric's explanations of [building rustc](https://www.reddit.com/r/rust/comments/5ag60z/how_do_i_bootstrap_rust_to_crosscompile_for_a_new/) with notes on used repos, namely `rust-buildbot` and crosstool-ng examples.

Crosstool for gcc toolchain:  `git clone git@github.com:asymptotik/crosstool-arm-osx.git` -- this is shit, avoid!

`https://stackoverflow.com/questions/50955843/linking-with-arm-linux-gnueabihf-gcc-failed-when-cross-compiling-a-rust-applic`

Crosstool for llvm toolchain: https://medium.com/@zw3rk/making-a-raspbian-cross-compilation-sdk-830fe56d75ba

```
./configure --prefix="/Users/berkus/Hobby/Metta/cross-tools/rpi3-cross-llvm/prebuilt" \
            --target=arm-linux-gnueabihf \
            --enable-gold=yes \
            --enable-ld=yes \
            --enable-targets=arm-linux-gnueabihf \
            --enable-multilib \
            --enable-interwork \
            --disable-werror \
            --quiet
make && make install
```

```
mkdir sdkroot
rsync -rzLR --safe-links \
      pi@172.20.10.2:/usr/lib/arm-linux-gnueabihf \
      pi@172.20.10.2:/usr/lib/gcc/arm-linux-gnueabihf \
      pi@172.20.10.2:/usr/include \
      pi@172.20.10.2:/lib/arm-linux-gnueabihf \
      sysroot/
```

`cp /Users/berkus/Hobby/Metta/cross-tools/rpi3-cross-llvm/sysroot/usr/lib/gcc/arm-linux-gnueabihf/6.3.0/crt* /Users/berkus/Hobby/Metta/cross-tools/rpi3-cross-llvm/prebuilt/arm-linux-gnueabihf/lib/`

`cp -r /Users/berkus/Hobby/Metta/cross-tools/rpi3-cross-llvm/sysroot/usr/lib/gcc /Users/berkus/Hobby/Metta/cross-tools/rpi3-cross-llvm/prebuilt/arm-linux-gnueabihf/lib/`

`set -x PATH /Users/berkus/Hobby/Metta/cross-tools/rpi3-cross-llvm/prebuilt/bin $PATH`
`./x.py build --host armv7-unknown-linux-gnueabihf --target aarch64-unknown-none --stage 1 src/libtest`

