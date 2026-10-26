//! 构建期选择 TapTap 配置：优先读取 gitignore 的 `taptap.local.json`（真实凭据），
//! 不存在则回落到仓库里的空模板 `taptap_config.json`，保证新克隆可直接编译。

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=taptap.local.json");
    println!("cargo:rerun-if-changed=taptap_config.json");

    let local = Path::new("taptap.local.json");
    let template = Path::new("taptap_config.json");
    let src = if local.exists() { local } else { template };

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR 未设置");
    std::fs::copy(src, Path::new(&out_dir).join("taptap_config.json"))
        .expect("复制 taptap 配置失败");
}
