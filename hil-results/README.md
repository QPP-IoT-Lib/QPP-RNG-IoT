# Resultados de las pruebas con hardware (HIL)

Este directorio guarda los datos de las pruebas *hardware-in-the-loop* de
QPP-RNG: las capturas hechas en las placas, los flujos de bytes derivados de
ellas y las salidas de las herramientas estadísticas. Aquí se explica cómo se
configuró cada ejecución y qué contiene cada archivo. Los resultados y su
análisis están en [`REPORT.md`](REPORT.md); el firmware y las herramientas de
captura, en [`../hil-firmware/`](../hil-firmware/README.md).

## Banco de pruebas

| Equipo | Hardware | Conexión | Fuente física de entropía |
|---|---|---|---|
| Arduino Mega 2560 | ATmega2560, 16 MHz | USART0, reinicio por DTR | Watchdog (oscilador RC de 128 kHz) medido con Timer1 |
| Arduino Nano | ATmega328P, 16 MHz, FT232R | USART0, reinicio por DTR | Igual que el Mega 2560 |
| makerdiary nRF52840 MDK | Cortex-M4F, 64 MHz | UART0 hacia DAPLink, reinicio por `probe-rs` o por *serial break* | RNG interno con corrección de sesgo; también se evaluó LFRC frente a CYCCNT |
| Raspberry Pi 4 | Cortex-A72, Debian 13 aarch64 | SSH | Ninguna adicional: temporización del generador de referencia (`CNTVCT_EL0`, 54 MHz) |
| PC con Windows 11 | Intel Core Ultra 7 265KF | Local | Igual que la Pi: `QueryPerformanceCounter`, 10 MHz |
| ESP32 | ESP32-D0WD-V3, 240 MHz | UART0 hacia CH340, reinicio por RTS | RNG interno con la fuente de ruido del ADC activada; también se evaluó el reloj lento RTC frente al cristal |

Las capturas de `raw/`, las capturas de fuente cruda de los AVR y todas las del
nRF52840 se hicieron el 2026-10-08 en un Mac. Las capturas de salida de 1 MB de
los AVR, el análisis de esas capturas y las ejecuciones de `e2e/` se hicieron
entre el 2026-10-09 y el 2026-10-11 en el PC con Windows, con las herramientas
NIST instaladas en WSL Ubuntu.

## Cómo se configuraron las ejecuciones

### Firmware y protocolo

- **Compilación:** perfil `release` con `opt-level = "s"`, LTO y
  `codegen-units = 1`. `qpp-rng-reference` y `entropy-timer` se compilan con
  `opt-level = 0`, como exige el diseño de referencia. Las únicas excepciones
  son `raw/nano-ref-opts-*`, que usan `opt-level = "s"` también en la
  referencia, y `e2e/`, que usa `opt-level = "z"`.
- **Puerto serie:** 115200 baudios, 8N1.
- **Formato en la placa:** una línea ASCII que empieza por `QPPHIL`, seguida de
  registros de `0xA5` más dos `u32` en *little-endian*.
- **Captura:** `capture.py` descarta el byte `0xA5`, de modo que cada registro
  ocupa 8 bytes en el `.bin`. Cada captura comienza justo después de un
  reinicio de la placa.

### Modos de firmware

El contenido de los dos campos de cada registro depende del modo:

| Modo | Encabezado | Campo 1 | Campo 2 |
|---|---|---|---|
| `ref` (referencia) | `QPPHIL <placa> ref-xorshift128plus N=5` | `n_p` del ciclo | `Δt` del ciclo, en ticks |
| `rawsrc` en AVR | `QPPHIL avr rawsrc wdt-vs-timer1` | Periodo del watchdog, en ciclos | Marca de tiempo absoluta del Timer1 |
| `rawsrc` en nRF52840 | `QPPHIL nrf52840 rawsrc lfrc-rtc32-vs-cyccnt+hwrng` | Ciclos entre 32 flancos del LFRC | Un byte del RNG interno |
| `iot` | `QPPHIL <placa> iot-… N=6 credit=…` | 4 bytes de salida | Ticks empleados en generarlos |

| Parámetro | `ref` | `iot` en AVR | `iot` en nRF52840 |
|---|---|---|---|
| Generador | `QppRngXorshift` | `QppRngIot` | `QppRngIot` |
| Tamaño de la permutación | `N = 5` | `N = 6` | `N = 6` |
| Semilla | Fija: `0x5EED0000111122223333444455556666` | Procede de la fuente física | Procede de la fuente física |
| Agrupación | `oversample = 1`: se ve cada ciclo | 4 recorridos por byte | 4 recorridos por byte |
| Entropía acreditada | No aplica | 1 bit por muestra | 4 bits por byte del RNG |
| Muestras por byte | No aplica | 8 | 4 |
| Pruebas de salud | Ninguna | RCT y APT de SP 800-90B (W = 512, α = 2⁻²⁰) | RCT y APT de SP 800-90B (W = 512, α = 2⁻²⁰) |

La salida por defecto de la referencia (`oversample = 5`) se reconstruye con
`extract.py ref`, que aplica XOR a 5 valores consecutivos de `n_p mod 256`.

### Tamaños de captura

| Ejecución | Registros | Finalidad |
|---|---|---|
| Repetibilidad de `ref` | 200–300 en AVR, 2.000 en nRF52840 | Comparar la salida entre reinicios |
| Repetibilidad de `iot` | 10 en AVR, 50 en nRF52840 | Comparar los primeros 40 o 200 bytes entre reinicios |
| Fuente cruda corta | 2.000–20.000 | Primera estimación de entropía |
| Salida `iot` corta | 2.500 en AVR (10 kB), 12.500 en nRF52840 (50 kB) | Primera batería estadística |
| Fuente cruda larga | 1.000.001 | 1 M de diferencias para SP 800-90B; el primer registro no tiene predecesor |
| Salida `iot` larga | 250.000 (1.000.000 bytes) | Batería completa |

Las capturas de 1 MB tardaron 2,75 h en el nRF52840, 37,7 h en el Mega 2560 y
36,9 h en el Nano. En los AVR se dividieron en 13 tramos (12 de 20.000 registros
y 1 de 10.000), cada uno iniciado con un reinicio de la placa. Los tramos se
concatenaron por orden de marca de tiempo.

### Análisis

| Herramienta | Comando | Dónde se aplica |
|---|---|---|
| NIST SP 800-90B, vía no IID | `ea_non_iid -v <archivo> 8` | Fuentes crudas y salidas |
| NIST SP 800-90B, vía IID | `ea_iid -v <archivo> 8` | Salidas y RNG del nRF52840 |
| Batería del repositorio (Tier 1, `ent`, SP 800-90B y SP 800-22) | `stats-cli full --dir <dir> --out <json>` | Salidas de 1 MB; en `raw/` se añadió `--skip-sp800-22` |
| NIST SP 800-22 (STS) | Lo ejecuta `stats-cli`: 8 secuencias de 1 Mbit | Salidas de 1 MB |
| Comparación con ascon-c 446347f | `qpp-iot/scripts/e2e-check.py --runs 3` | `e2e/` |

Las secuencias de entropía de `raw/` tienen menos de 1 M de muestras, por lo que
sus resultados son orientativos. Los valores que deben citarse son los de
`long/`.

## Contenido del directorio

```text
hil-results/
├── README.md        este archivo
├── REPORT.md        informe completo, con resultados y análisis
├── raw/             capturas cortas del 2026-10-08 y sus análisis
├── samples/         flujos de bytes derivados de raw/
├── long/            capturas de 1 M de muestras y 1 MB, y sus análisis
├── pi4/             referencia en Raspberry Pi 4 y Windows
├── e2e/             pila QPP-IoT completa en las placas
├── esp32/           todos los datos de la placa ESP32
└── flash-backups/   firmware original de las placas
```

### Tipos de archivo

| Patrón | Contenido |
|---|---|
| `*.bin` en `raw/`, o `<placa>-{rawsrc,iot}.bin` en `long/` | Registros de 8 bytes (dos `u32` *little-endian*), según el modo |
| `*.meta` | Encabezado del firmware, número de registros, bytes de sincronización erróneos, duración, registros por segundo y, en las capturas largas, `status=` |
| `ea_non_iid-*.txt`, `ea_iid-*.txt` | Salida completa de la herramienta NIST correspondiente |
| `stats-*.json` | Informe de `stats-cli`, con una entrada por candidato |
| `sts-finalAnalysisReport-*.txt` | Informe final de NIST STS (SP 800-22) |

El campo `status=` solo aparece en las capturas hechas con la versión actual de
`capture.py`, y sus valores son `complete`, `timeout`, `stalled`,
`health-test-failure` o `serial-error: …`. Los `.meta` de `raw/` son anteriores
y no lo incluyen. Todas las capturas registradas tienen `bad_sync_bytes=0`.

### `raw/`

Los nombres siguen el patrón `<placa>-<modo>-<ejecución>`, con `mega`, `nano` o
`nrf` como placa.

| Archivos | Contenido |
|---|---|
| `*-ref-repro-{a,b,c}` | Referencia tras reinicios distintos; se compara si la salida se repite |
| `mega-ref-probe` | Primera prueba de 200 registros de la referencia en el Mega 2560 |
| `nano-ref-opts-{a,b}` | Referencia en el Nano compilada con `opt-level = "s"` en lugar de `0` |
| `*-rawsrc-{a,b}` | Fuente física cruda: dos capturas independientes |
| `*-iot-repro-{a,b}` | Salida `iot` tras dos reinicios |
| `*-iot-long` | Salida `iot` usada en la primera batería (10 kB en AVR, 50 kB en nRF52840) |
| `ea_*-<placa>-iot.txt` | Análisis NIST de la salida corta |
| `ea_*-<placa>-{wdt,lfrc}-jitter-lowbyte.txt`, `ea_*-nrf52840-hwrng-raw.txt` | Análisis NIST de la fuente corta |
| `rawsrc-entropy-{avr,nrf}.txt` | Estadísticas y estimación rápida con `tools/entropy.py` (subconjunto parcial de SP 800-90B) |
| `stats-cli-full-iot.json` | `stats-cli full --skip-sp800-22` sobre las salidas cortas |

### `samples/`

Secuencias de bytes obtenidas de `raw/` con `extract.py`, que son la entrada de
los análisis.

| Archivo | Tamaño | Origen |
|---|---|---|
| `<placa>-iot.bin` | 10.000 B en AVR, 50.000 B en nRF52840 | Salida del generador IoT |
| `<placa>-wdt-jitter-lowbyte.bin` | 19.999 B | Byte bajo del periodo del watchdog |
| `nrf52840-lfrc-jitter-lowbyte.bin` | 19.999 B | Byte bajo de la medida LFRC frente a CYCCNT |
| `nrf52840-hwrng-raw.bin` | 20.000 B | Bytes directos del RNG interno |
| `nrf52840-ref.bin` | 400 B | Salida por defecto reconstruida de la referencia |

### `long/`

| Archivos | Contenido |
|---|---|
| `<placa>-rawsrc.{bin,meta}` | 1.000.001 registros de fuente cruda |
| `<placa>-wdt-jitter-lowbyte-1M.bin` | 1 M de bytes bajos del periodo del watchdog |
| `nrf52840-lfrc-jitter-lowbyte-1M.bin`, `nrf52840-hwrng-raw-1M.bin` | Las dos fuentes del nRF52840 |
| `<placa>-iot.bin` | 250.000 registros de salida; en AVR, la unión de los 13 tramos |
| `<placa>-iot-1MB.bin` | Los 1.000.000 de bytes de salida extraídos |
| `nrf52840-iot.meta` | Metadatos de la captura `iot` larga del nRF52840 |
| `{mega2560,nano}-iot-chunks/` | Un `.meta` por tramo y el registro del proceso de captura (`<placa>-driver.log`) |
| `<placa>.log` | Registro de `long-capture.sh` en el Mac |
| `ea_non_iid-<placa>-{wdt,lfrc,hwrng-raw}-1M.txt`, `ea_iid-nrf52840-hwrng-raw-1M.txt` | Análisis NIST de la fuente con 1 M de muestras |
| `ea_{iid,non_iid}-{mega2560,nano}-iot-1MB.txt` | Análisis NIST de la salida de 1 MB en AVR |
| `stats-full-avr-iot.json`, `stats-full-nrf52840-iot.json` | Batería completa sobre las salidas de 1 MB |
| `sts-finalAnalysisReport-<placa>-iot.txt` | Detalle de SP 800-22 por placa |
| `stats-cli.log` | Salida de `stats-cli` en la ejecución de los AVR |

Los análisis de la salida de 1 MB del nRF52840 están en
`stats-full-nrf52840-iot.json`; no hay archivos `ea_*-nrf52840-iot-1MB.txt`.

### `pi4/`

Generador de referencia en sistemas rápidos, medido con
`crates/qpp-rng-firmware/examples/pi_trace.rs` (mismo formato de registro `ref`).

| Archivos | Contenido |
|---|---|
| `ref-{a,b,c}.bin` | 3 trazas de 20.000 ciclos en la Pi |
| `pi4-ref-1MB.bin` | Salida reconstruida de la referencia, a partir de 5.000.005 ciclos |
| `pi4-ref-dt-modres-5M.bin`, `win-ref-dt-modres-2M.bin` | `Δt − moda(Δt \| n_p) + 128` por ciclo, en la Pi y en Windows |
| `ea_non_iid-{pi4,win}-dt-modres-1M.txt` | Evaluación NIST de esos residuos: 0,375 y 0,407 bits por ciclo |
| `ea_non_iid-pi4-dt-linfit-residual-1M.txt` | Evaluación con residuo de un ajuste lineal global. **Entrada no válida**: se conserva solo como registro (véase `REPORT.md`, §5) |
| `samples/` | 1 MB por candidato de la biblioteca, generados en la Pi |
| `stats-full-pi4.json` | Batería ejecutada en la Pi. **Su columna SP 800-22 no es válida**; Tier 1, `ent` y SP 800-90B sí lo son |
| `stats-full-pi4-samples-wsl.json` | Repetición correcta sobre `samples/`, ejecutada en WSL |
| `stats-full-pi4-ref-1MB-wsl.json`, `sts-finalAnalysisReport-pi4-ref-1MB-wsl.txt` | Repetición correcta sobre `pi4-ref-1MB.bin` |
| `cargo-test-summary.txt` | Resultado de `cargo test --workspace --release` en la Pi |

### `e2e/`

Ejecución de la pila completa en cada placa: fuente física → QPP-RNG → SHA-256
→ Ascon-AEAD128. El firmware es `qpp-iot/firmware/e2e`.

Cada ejecución genera una clave y un nonce, y cifra y descifra mensajes de 0, 16
y 64 bytes con 16 bytes de datos asociados. Además, comprueba que se rechace una
copia con un bit alterado. Se hicieron 3 reinicios por placa, y cada texto
cifrado y etiqueta se comparó con la referencia ascon-c.

| Archivos | Contenido |
|---|---|
| `<placa>-e2e.log` | Informe de los 3 reinicios: tiempos en ciclos, uso de pila y `RESULT=` |
| `e2e-<chip>.hex` | Imagen exacta que se ejecutó |
| `SHA256SUMS` | Sumas de comprobación de las imágenes |

### `flash-backups/`

Contenido original de cada placa, guardado antes de grabar el firmware de
prueba. Los comandos de restauración están en `REPORT.md`, sección
«Reproducing».

| Archivo | Placa |
|---|---|
| `mega2560-original.hex` | Arduino Mega 2560 |
| `nano328p-original.hex` | Arduino Nano |
| `nrf52840-original.bin` | nRF52840 MDK |

## ESP32

Los datos están en `esp32/`, que repite la organización de los demás
directorios. Placa: ESP32-D0WD-V3 a 240 MHz, con cristal de 40 MHz, 4 MB de
flash y CH340 en `COM4`. Las capturas se hicieron el 2026-10-30 en el PC con
Windows, y los análisis, en WSL Ubuntu. Los resultados están en `REPORT.md`, §7.

### Configuración

- **Firmware:** `../hil-firmware/esp32`, con `esp-hal` 1.2.2 para el arranque,
  los relojes y la UART. Usa el mismo formato de registro y los mismos perfiles
  de compilación que las demás placas. Se ejecuta en el núcleo 0, con el núcleo
  1 detenido y la radio apagada.
- **Reinicio:** `capture.py --reset-rts`, por el pin EN a través del puente
  USB-serie. No se probaron ciclos de alimentación.
- **Encuadre estricto:** `capture.py` solo acepta un registro si le sigue el
  byte de sincronización del siguiente. Los rechazos se cuentan en
  `dropped_frames`. Las capturas de `esp32/long/` y las `esp32-iot-repro-*` ya
  se hicieron con esta comprobación, sin ningún rechazo.

| Modo | Encabezado | Campo 1 | Campo 2 |
|---|---|---|---|
| `ref` | `QPPHIL esp32 ref-xorshift128plus N=5` | `n_p` | `Δt`, en ciclos de CPU |
| `rawsrc` | `QPPHIL esp32 rawsrc rtcslow256-vs-xtal+hwrng-adc` | Ciclos del cristal de 40 MHz en 256 ciclos del reloj lento RTC | Palabra de 32 bits del RNG, con la fuente de ruido del ADC activada |
| `rawsrc-noadc` | `QPPHIL esp32 rawsrc rtcslow256-vs-xtal+hwrng-noadc` | Igual que `rawsrc` | Palabra del RNG, con esa fuente desactivada |
| `iot` | `QPPHIL esp32 iot-hwrng N=6 credit=4 samples_per_byte=4 …` | 4 bytes de salida | Ciclos de CPU empleados |

La versión `iot` usa como fuente el byte bajo del RNG con la fuente de ruido del
ADC activada, y le acredita 4 bits por byte. Esa acreditación se apoya en la
evaluación de 7,15 bits por byte con 1 M de muestras.

| Ejecución | Registros | Archivos |
|---|---|---|
| Repetibilidad de `ref` | 2.000 × 2 y 20.000 × 3 | `raw/esp32-ref-repro-{a,b}`, `raw/esp32-ref-long-{a,b,c}` |
| Fuente cruda corta | 20.000 × 2 | `raw/esp32-rawsrc-{a,b}` |
| RNG sin la fuente de ruido del ADC | 20.000 × 2 | `raw/esp32-rawsrc-noadc-{a,b}` |
| Repetibilidad de `iot` | 50 × 2 (200 bytes) | `raw/esp32-iot-repro-{a,b}` |
| Fuente cruda larga | 1.000.001 | `long/esp32-rawsrc` |
| Salida `iot` larga | 250.000 (1.000.000 bytes), en un solo arranque | `long/esp32-iot` |
| Pila completa | 3 reinicios | `e2e/esp32-e2e.log` |

### Contenido de `esp32/`

| Archivos | Contenido |
|---|---|
| `raw/*.{bin,meta}` | Capturas cortas, en el formato de registros descrito arriba |
| `raw/ea_*-esp32-*.txt` | Análisis NIST de las muestras cortas |
| `samples/esp32-ref.bin` | 4.000 B de salida reconstruida de la referencia, a partir de `esp32-ref-long-a` |
| `samples/esp32-rtcslow-jitter-lowbyte.bin` | 19.999 B: byte bajo de la medida del reloj lento RTC |
| `samples/esp32-hwrng-raw.bin`, `samples/esp32-hwrng-noadc-raw.bin` | 20.000 B del RNG, con y sin la fuente de ruido del ADC |
| `long/esp32-rawsrc.{bin,meta}`, `long/esp32-iot.{bin,meta}` | Capturas largas en formato de registros |
| `long/esp32-rtcslow-jitter-lowbyte-1M.bin`, `long/esp32-hwrng-raw-1M.bin` | Las dos fuentes con 1 M de muestras, extraídas de `esp32-rawsrc` |
| `long/esp32-iot-1MB.bin` | Los 1.000.000 de bytes de salida |
| `long/ea_*-esp32-{rtcslow,hwrng-raw}-1M.txt` | Análisis NIST de las fuentes |
| `long/ea_*-esp32-iot-1MB.txt` | Análisis NIST de la salida |
| `long/stats-full-esp32-iot.json`, `long/stats-cli.log` | Batería completa y su registro |
| `long/sts-finalAnalysisReport-esp32-iot.txt` | Detalle de SP 800-22 |
| `e2e/esp32-e2e.log`, `e2e/e2e-esp32.bin`, `e2e/SHA256SUMS` | Informe de los 3 reinicios, imagen exacta y su suma de comprobación |

El firmware original está respaldado localmente en
`flash-backups/esp32-original.bin`, pero **ese archivo no se versiona**: la
partición NVS contiene entradas de configuración Wi-Fi que podrían incluir
credenciales.

Los tiempos de cifrado de `e2e/esp32-e2e.log` no son comparables con los de las
demás placas: el código se ejecuta desde la flash con la caché fría, de modo que
el primer cifrado cuesta 17 veces más que los siguientes (véase `REPORT.md`, §7).

## Reproducción

Los comandos de compilación, grabación, captura y análisis están en
`REPORT.md` («Reproducing») y en `../hil-firmware/README.md`. Las capturas
largas en Windows se describen en `../hil-firmware/portable/README.md`.

Al ejecutar STS, el directorio real de `assess` debe estar en `PATH`, sin usar
un enlace simbólico: de lo contrario, los resultados de SP 800-22 no son
válidos. Conviene revisar además que `stats.txt` no contenga `ABORTED`.
