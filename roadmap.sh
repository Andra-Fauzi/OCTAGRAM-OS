#!/usr/bin/env bash
# =============================================================================
#  roadmap.sh — OCTAGRAM-OS Roadmap Feature Test Runner
#  Menguji semua fitur Priority 1 & 2 yang terdaftar di ROADMAP.md
# =============================================================================
set -uo pipefail

# ── Warna & helper ──────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
CYAN='\033[0;36m'; BOLD='\033[1m'; RESET='\033[0m'

PASS=0; FAIL=0; SKIP=0
FAILURES=()

section() { echo -e "\n${CYAN}${BOLD}══ $* ══${RESET}"; }
info()    { echo -e "   ${YELLOW}ℹ${RESET}  $*"; }
ok()      { echo -e "   ${GREEN}✓${RESET}  $*"; PASS=$((PASS+1)); }
fail()    { echo -e "   ${RED}✗${RESET}  $*"; FAIL=$((FAIL+1)); FAILURES+=("$*"); }
skip()    { echo -e "   ${YELLOW}⊘${RESET}  $* ${YELLOW}[butuh QEMU]${RESET}"; SKIP=$((SKIP+1)); }

# Jalankan satu kernel unit test berdasarkan nama pendek (tanpa modul prefix)
run_kernel_test() {
    local name="$1"
    if cargo test --manifest-path kernel/Cargo.toml \
           --target x86_64-unknown-linux-gnu \
           -- --exact "$name" 2>&1 | grep -qE "^test .* ok$|1 passed"; then
        ok "$name"
    else
        fail "$name"
    fi
}

# Jalankan satu kernel unit test dengan nama lengkap (modul::tests::nama)
run_kernel_test_full() {
    local full="$1"
    local label="${2:-$full}"
    if cargo test --manifest-path kernel/Cargo.toml \
           --target x86_64-unknown-linux-gnu \
           -- --exact "$full" 2>&1 | grep -qE "test .+ ok|1 passed"; then
        ok "$label"
    else
        fail "$label"
    fi
}

# Jalankan perintah build/make dan laporkan hasilnya
run_build() {
    local label="$1"; shift
    if "$@" > /tmp/roadmap_build.log 2>&1; then
        ok "$label"
    else
        fail "$label (lihat /tmp/roadmap_build.log)"
    fi
}

# =============================================================================
echo -e "${BOLD}"
echo "╔════════════════════════════════════════════════════════════╗"
echo "║          OCTAGRAM-OS  —  Roadmap Test Runner               ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo -e "${RESET}"

# =============================================================================
section "Build — Kernel (x86_64)"
# =============================================================================
run_build "make -C kernel KARCH=x86_64" make -C kernel KARCH=x86_64

# =============================================================================
section "Build — User ELF programs"
# =============================================================================
run_build "make -C user" make -C user

# =============================================================================
section "PRIORITY 1 — Core Process Isolation"
section "P1.1 · MAP_SHARED / device mapping fork inheritance"
# =============================================================================
run_kernel_test_full \
    "elf::tests::fork_does_not_inherit_memory_locks" \
    "fork tidak mewarisi kunci memori (mlockall reset)"

run_kernel_test_full \
    "elf::tests::fork_shared_pages_filter_includes_shared_backings_and_device_mappings" \
    "fork menyalin MAP_SHARED & device mapping tanpa alokasi ganda"

# =============================================================================
section "P1.2 · Credential & Capability enforcement"
# =============================================================================
run_kernel_test_full \
    "elf::tests::credentials_and_capabilities_follow_uid_rules" \
    "kredensial uid/euid/gid/egid dan kapabilitas mengikuti aturan root/non-root"

run_kernel_test_full \
    "elf::tests::capability_checks_require_the_requested_bit" \
    "CAP_VFS_WRITE, CAP_NETWORK, CAP_SCHED_NICE diperiksa per bit"

run_kernel_test_full \
    "elf::tests::device_and_console_access_respects_credentials_and_capabilities" \
    "device_access_allowed: /dev/fb0, tty konsol, descriptor lama diblokir setelah uid drop"

# =============================================================================
section "P1.3 · Address range & ELF segment validation"
# =============================================================================
run_kernel_test_full \
    "elf::tests::user_range_validation_rejects_overflow_and_non_user_addresses" \
    "user_range_validation: panjang 0, di bawah USER_MIN, melewati USER_MAX, overflow"

run_kernel_test_full \
    "elf::tests::rejects_load_segments_outside_the_user_address_space" \
    "PT_LOAD di luar user space ditolak"

run_kernel_test_full \
    "elf::tests::rejects_malformed_load_segments" \
    "PT_LOAD malformed (W+X, align buruk, filesz>memsz, file terpotong) ditolak"

run_kernel_test_full \
    "elf::tests::mapping_size_rounds_pages_and_rejects_bad_lengths" \
    "mapping_size: pembulatan halaman, panjang nol/overflow ditolak"

# =============================================================================
section "P1.4 · Tests yang butuh QEMU (tidak bisa di host)"
# =============================================================================
skip "Page fault nyata pada mapping yang sudah di-munmap"
skip "Isolasi akses memori lintas proses (cross-process memory access)"

# =============================================================================
section "PRIORITY 2 — POSIX Runtime"
section "P2.1 · POSIX spawn: file actions addchdir / addchdir_np"
# =============================================================================
# Build check sudah dilakukan di atas (make -C user).
# Demo binary posix_spawn_demo berisi cabang chdir-child & setsid-child.
if [ -f user/build/posix_spawn_demo.elf ] || [ -f user/posix_spawn_demo ]; then
    ok "posix_spawn_demo ELF ada (addchdir / setsid cabang tersedia)"
else
    fail "posix_spawn_demo ELF tidak ditemukan (make -C user mungkin gagal)"
fi

# =============================================================================
section "P2.2 · Scheduler — RT priorities & policies"
# =============================================================================
run_kernel_test_full \
    "elf::tests::scheduler_policies_accept_rt_priorities" \
    "SCHED_FIFO/SCHED_RR menerima rt_priority 1..99"

run_kernel_test_full \
    "elf::tests::rt_tasks_outrank_sched_other_and_higher_rt_priority_wins" \
    "task RT mengungguli SCHED_OTHER; RT priority tinggi menang"

run_kernel_test_full \
    "elf::tests::fifo_holds_ties_while_rr_rotates" \
    "SCHED_FIFO tahan seri; SCHED_RR rotasi quantum RT_RR_QUANTUM_TICKS"

run_kernel_test_full \
    "elf::tests::sched_policy_changes_need_capability_or_self_demotion" \
    "non-root hanya boleh turun ke SCHED_OTHER tanpa CAP_SCHED_NICE"

# =============================================================================
section "P2.3 · Scheduler — Nice & aging anti-starvasi"
# =============================================================================
run_kernel_test_full \
    "elf::tests::scheduler_prefers_higher_nice_priority" \
    "nice lebih rendah (prioritas tinggi) dipilih lebih dulu"

run_kernel_test_full \
    "elf::tests::scheduler_aging_prevents_starvation" \
    "aging +1 tiap dilewati, boost maksimum 39 level, mencegah starvasi"

run_kernel_test_full \
    "elf::tests::starvation_watchdog_trips_after_the_aging_budget" \
    "penanda WATCHDOG trip saat age >= 78"

run_kernel_test_full \
    "elf::tests::wall_clock_watchdog_and_clock_ticks_follow_the_thresholds" \
    "watchdog waktu nyata: wait >= 500 ms, cpu_ms dan wait_ms dicatat"

# =============================================================================
section "P2.4 · TTY / console scheduler-aware"
# =============================================================================
run_kernel_test_full \
    "fs::devfs::tests::canonical_eof_is_reported_once_when_the_line_is_empty" \
    "Ctrl-D baris kosong → eof_pending, read()=0, poll() siap"

run_kernel_test_full \
    "fs::devfs::tests::tty_path_terminal_matches_readable_terminals_only" \
    "tty_path_terminal() cocok hanya untuk terminal yang bisa dibaca"

# =============================================================================
section "P2.5 · Futex: robust list & bitset wake"
# =============================================================================
run_kernel_test_full \
    "elf::tests::robust_list_marks_only_the_owner_as_dead" \
    "release_robust_list: hanya pemilik ditandai FUTEX_OWNER_DIED"

run_kernel_test_full \
    "elf::tests::futex_wake_matches_only_intersecting_bitsets" \
    "FUTEX_WAKE_BITSET hanya membangunkan waiter dengan bitset berpotongan"

# =============================================================================
section "Host C runtime tests"
# =============================================================================
run_build "scanf runtime (make -C user scanf-test)"  make -C user scanf-test
run_build "atexit/cxa_finalize (make -C user cxa-test)" make -C user cxa-test
run_build "DNS resolver (make -C user resolv-test)"  make -C user resolv-test
run_build "IPv6 parsing (make -C user ipv6-test)"    make -C user ipv6-test

# =============================================================================
section "Kernel unit tests — full suite"
# =============================================================================
info "Menjalankan seluruh suite kernel host tests..."
TOTAL_LINE=$(cargo test --manifest-path kernel/Cargo.toml \
    --target x86_64-unknown-linux-gnu 2>&1 \
    | grep -E "^test result:")
if echo "$TOTAL_LINE" | grep -q "FAILED"; then
    fail "Ada kegagalan di suite lengkap kernel: $TOTAL_LINE"
else
    PASSED_N=$(echo "$TOTAL_LINE" | grep -oP '\d+ passed' | grep -oP '\d+')
    ok "Semua ${PASSED_N:-?} kernel unit tests lulus"
fi

# =============================================================================
#  RINGKASAN
# =============================================================================
echo ""
echo -e "${BOLD}════════════════════════════════════════${RESET}"
printf "${BOLD}Hasil: ${GREEN}%d lulus${RESET}  ${RED}%d gagal${RESET}  ${YELLOW}%d dilewati (QEMU)${RESET}\n" \
    "$PASS" "$FAIL" "$SKIP"

if [ ${#FAILURES[@]} -gt 0 ]; then
    echo -e "\n${RED}${BOLD}Item yang gagal:${RESET}"
    for f in "${FAILURES[@]}"; do
        echo -e "  ${RED}✗${RESET} $f"
    done
fi

echo -e "${BOLD}════════════════════════════════════════${RESET}"

# Keluar dengan kode 1 jika ada kegagalan
[ "$FAIL" -eq 0 ]
