# Cosmwasmception

## Running

Fastest way to run this contract would be through our [vm](git@github.com:ComposableFi/cosmwasm-vm.git). But note that our vm runs an already built version this contract, so follow building guidelines to build and run if you want to make any changes.

Clone the vm.
```sh
# Clone our vm
git clone git@github.com:ComposableFi/cosmwasm-vm.git
# Workshop branch contains some modifications to be able
# to upload and run contracts that are over 200kb xd.
git checkout workshop
```

Run it.
```sh
cargo test --profile=release --features=iterator --package cosmwasm-vm-wasmi semantic::test_workshop -- --exact
```

### Few important notes

- `release` profile should be enabled, otherwise it will take a lot to run.
- The test to run the workshop is `test_workshop` in `vm-wasmi/src/semantic.rs`, you can tweak it however you like.

## Building

```
RUSTFLAGS='-C link-arg=-s' cargo b --target=wasm32-unknown-unknown --profile release
```
- `RUSTFLAGS` are used to reduce the binary size.

- Our vm uses the contract in its `fixtures` directory. So either copy the output contract to there:
```
cp $PATH_TO_COSMWASMCEPTION/target/wasm32-unknown-unknown/release/cosmwasmception.wasm $PATH_TO_COSMWASM_VM/fixtures/
```
Or just change the file path in `test_workshop`.
