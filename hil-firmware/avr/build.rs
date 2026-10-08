fn main() {
    let mcu = std::env::var("AVR_MCU")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "atmega328p".to_string());
    cc::Build::new()
        .file("c/wdt_jitter.c")
        .flag(format!("-mmcu={mcu}"))
        .opt_level_str("s")
        .compile("qpp_wdt_jitter");
    println!("cargo:rerun-if-changed=c/wdt_jitter.c");
    println!("cargo:rerun-if-env-changed=AVR_MCU");
}
