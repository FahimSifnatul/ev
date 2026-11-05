## $ cargo check --all-targets --target x86_64-pc-windows-gnu
    Checking ev v0.1.0 (/Users/pathaoltd/codes/sifnatul/ev)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.34s
exit=0
## $ cargo clippy --all-targets --target x86_64-pc-windows-gnu -- -D warnings
    Checking ev v0.1.0 (/Users/pathaoltd/codes/sifnatul/ev)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.24s
exit=0
## $ cargo clippy --all-targets --locked -- -D warnings
    Checking ev v0.1.0 (/Users/pathaoltd/codes/sifnatul/ev)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.09s
exit=0
## $ cargo test --locked
   Compiling ev v0.1.0 (/Users/pathaoltd/codes/sifnatul/ev)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.31s
     Running unittests src/main.rs (target/debug/deps/ev-d5414a7ad3cafc60)

running 15 tests
test danger::tests::match_is_case_insensitive ... ok
test danger::tests::ordinary_name_is_safe ... ok
test danger::tests::prefixes_are_dangerous ... ok
test store::tests::accepts_valid_keys ... ok
test store::tests::accepts_plain_values ... ok
test danger::tests::exact_name_is_dangerous ... ok
test store::tests::rejects_control_characters_in_values ... ok
test store::tests::rejects_invalid_keys ... ok
test store::unix::tests::hook_lines_carry_marker ... ok
test store::unix::tests::parses_set_and_unset_lines ... ok
test store::unix::tests::quotes_for_fish ... ok
test store::unix::tests::quotes_for_sh ... ok
test store::unix::tests::renders_env_files ... ok
test store::unix::tests::replace_overrides_existing_key ... ok
test store::unix::tests::serialize_round_trips ... ok

test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/cli.rs (target/debug/deps/cli-2c608971bc7fbbff)

running 13 tests
test get_missing_fails ... ok
test invalid_key_is_rejected ... ok
test init_keeps_non_utf8_rc_bytes ... ok
test init_adds_hook_once ... ok
test set_succeeds_when_rc_cannot_be_located ... ok
test env_sh_can_be_sourced ... ok
test critical_var_needs_yes ... ok
test set_hints_until_init ... ok
test state_files_are_owner_only ... ok
test set_overwrites ... ok
test unset_removes_and_records_marker ... ok
test set_then_get_and_list ... ok
test aliases_work ... ok

test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.61s

exit=0
## $ sh -n install.sh
exit=0
## temp-HOME install, EV_NO_MODIFY_PATH=1 (HOME=/private/tmp/claude-501/-Users-pathaoltd-codes-sifnatul-ev/1e9824aa-d51e-4c88-9ddf-709c32ac26fb/scratchpad/home.QFNO)
Building ev from source (a minute or two the first time)...
Installed ev 0.1.0 at /private/tmp/claude-501/-Users-pathaoltd-codes-sifnatul-ev/1e9824aa-d51e-4c88-9ddf-709c32ac26fb/scratchpad/home.QFNO/.local/bin/ev
Shell startup files left alone. Run 'ev init' yourself to load variables set with ev.
/private/tmp/claude-501/-Users-pathaoltd-codes-sifnatul-ev/1e9824aa-d51e-4c88-9ddf-709c32ac26fb/scratchpad/home.QFNO/.local/bin is not on your PATH. Add it yourself:  export PATH="/private/tmp/claude-501/-Users-pathaoltd-codes-sifnatul-ev/1e9824aa-d51e-4c88-9ddf-709c32ac26fb/scratchpad/home.QFNO/.local/bin:$PATH"

Ready. Try:   ev --help


ev is one self-contained program, so you can delete the folder you installed from.
--- files in temp HOME:
.
./.local
./.local/bin
./.local/bin/ev

## temp-HOME install, default (control)
Added $HOME/.local/bin to your PATH in /private/tmp/claude-501/-Users-pathaoltd-codes-sifnatul-ev/1e9824aa-d51e-4c88-9ddf-709c32ac26fb/scratchpad/home.B1Fx/.zshrc

Open a new terminal window, then run:   ev --help
(or in this window right now:   export PATH="/private/tmp/claude-501/-Users-pathaoltd-codes-sifnatul-ev/1e9824aa-d51e-4c88-9ddf-709c32ac26fb/scratchpad/home.B1Fx/.local/bin:$PATH")

Variables you set with ev (ev set NAME value) are loaded by every new terminal.

ev is one self-contained program, so you can delete the folder you installed from.
.
./.config
./.config/ev
./.config/ev/env.fish
./.config/ev/env.sh
./.config/ev/vars
./.local
./.local/bin
./.local/bin/ev
./.zshrc
