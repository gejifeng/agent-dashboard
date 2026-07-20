fn main() {
    // tauri-build 会把 icons/icon.ico 编进 Windows 资源段，但它没有为图标文件
    // 发出 rerun-if-changed。一旦它发出了其它 rerun-if-changed，cargo 的"包内
    // 任意文件变动就重跑"默认行为即失效，于是改了图标后 cargo build 不会重跑
    // 构建脚本，exe 里仍是旧图标（debug 和 release 都会中招）。
    println!("cargo:rerun-if-changed=icons");

    tauri_build::build()
}
