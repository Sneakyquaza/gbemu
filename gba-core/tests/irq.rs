//! IRQ VBlank servie par le BIOS libre : SWI VBlankIntrWait (HALT, vecteur 0x18, gestionnaire
//! utilisateur en 0x03007FFC, drapeaux en 0x03007FF8).

use gba_core::Gba;

/// Programme ARM assemblé à la main (adresses relatives à 0x08000000).
const PROGRAMME: [u32; 24] = [
    0xE3A0_0301, // 00 mov r0, #0x04000000
    0xE3A0_1008, // 04 mov r1, #8
    0xE1C0_10B4, // 08 strh r1, [r0, #4]      ; DISPSTAT : IRQ VBlank
    0xE280_2C02, // 0C add r2, r0, #0x200
    0xE3A0_1001, // 10 mov r1, #1
    0xE1C2_10B0, // 14 strh r1, [r2]          ; IE = VBlank
    0xE582_1008, // 18 str r1, [r2, #8]       ; IME = 1
    0xE28F_1018, // 1C add r1, pc, #0x18      ; r1 = gestionnaire (0x3C)
    0xE3A0_3403, // 20 mov r3, #0x03000000
    0xE283_3C7F, // 24 add r3, r3, #0x7F00
    0xE283_30FC, // 28 add r3, r3, #0xFC
    0xE583_1000, // 2C str r1, [r3]           ; [0x03007FFC] = gestionnaire
    0xEF05_0000, // 30 swi 0x05               ; VBlankIntrWait
    0xE285_5001, // 34 add r5, r5, #1
    0xEAFF_FFFC, // 38 b 0x30
    // Gestionnaire : acquitte IF et met le drapeau BIOS.
    0xE3A0_0301, // 3C mov r0, #0x04000000
    0xE280_0C02, // 40 add r0, r0, #0x200
    0xE3A0_1001, // 44 mov r1, #1
    0xE1C0_10B2, // 48 strh r1, [r0, #2]      ; IF = VBlank (acquitte)
    0xE3A0_0403, // 4C mov r0, #0x03000000
    0xE280_0C7F, // 50 add r0, r0, #0x7F00
    0xE280_00F8, // 54 add r0, r0, #0xF8
    0xE580_1000, // 58 str r1, [r0]           ; drapeaux IntrWait
    0xE12F_FF1E, // 5C bx lr
];

#[test]
fn vblank_intr_wait_une_fois_par_image() {
    let rom: Vec<u8> = PROGRAMME.iter().flat_map(|m| m.to_le_bytes()).collect();
    let mut gba = Gba::new(&rom, None).unwrap();
    for _ in 0..60 {
        gba.run_frame();
    }
    let n = gba.registres()[5];
    assert!(
        (58..=61).contains(&n),
        "{n} retours de VBlankIntrWait en 60 images"
    );
}
