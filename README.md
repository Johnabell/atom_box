# Atom Box

Atom Box provides an atomic box implementation where the box owns the contained value and its underlying allocation.
It is therefore, responsible for deallocating the memory when it is no longer being accessed by any threads.

Under the covers Atom Box is 'a safe implementation of hazard pointers in rust.'

The aim of this project has always been to provide a safe and idiomatic rust API for using hazard pointers for safe memory reclamation in multi-threaded concurrent lock-free data structures.

## Discussion

One of the difficulties of building lock free data structures in languages that do not have a garbage collector is the safe reclamation of allocated memory.
For example, if one thread atomically loads a pointer and another subsequently swaps the pointer, we need a mechanism for working out when the first thread is no longer accessing the pointer and we can reclaim the underlying memory.
One solution to this is using epoch based memory reclamation such as that provided by [Crossbeam](https://github.com/crossbeam-rs/crossbeam).
An alternative method is via use of hazard pointers which is the implementation used in this crate.

Many users of this crate will be able to make use of the `AtomBox` without a detailed understanding of the underlying mechanism.
However, there are a few important factors that should be considered when choosing to use this particular implementation:

- Every load requires the acquisition of a Hazard Pointer. An operation which is linear in the number of threads and number of active `AtomBox`es.
- There is a memory overhead associated with both the hazard pointers themselves and items that have been retired but are yet to be reclaimed.
- Hazard pointers will only be deallocated when a domain is dropped.
  In the case of the default shared domain, it is statically allocated and consequently never dropped.
  If this is undesirable behaviour, a custom domain can be used to ensure that hazard pointers are actually reclaimed.
  Alternatively, feel free to raise an issue for an API to be provided whereby un-used hazard pointers can be dropped.

For a more detailed discussion, see below.

### Hazard Pointer

Hazard pointers provide a safe memory reclamation method.
It protects objects from being reclaimed while being accessed by one or more threads, but allows objects to be removed concurrently while being accessed.

A hazard pointer is a single-writer multi-reader pointer that can be owned by at most one thread at a time.
To protect an object A from being reclaimed while in use, a thread X sets one of its owned hazard pointers, P, to the address of A.
If P is set to &A before A is removed (i.e., it becomes unreachable) then A will not be reclaimed as long as P continues to hold the value &A.

#### Memory Usage

- The size of the metadata for the library is linear in the number of threads using hazard pointers, assuming a constant number of hazard pointers per thread, which is typical.
- The typical number of retired but not yet reclaimed objects is linear in the number of hazard pointers, which typically is linear in the number of threads using hazard pointers.

#### Alternative Safe Reclamation Methods

- Locking (exclusive or shared):
  - Pros: simple to reason about.
  - Cons: serialization, high reader overhead, high contention, deadlock.
  - When to use: When speed and contention are not critical, and
    when deadlock avoidance is simple.
- Reference counting (`std::sync::Arc`):
  - Pros: automatic reclamation, thread-anonymous, independent of support for thread local data, immune to deadlock.
  - Cons: high reader (and writer) overhead, high reader (and writer) contention.
  - When to use: When thread local support is lacking and deadlock can be a problem, or automatic reclamation is needed.
- Read-copy-update (RCU):
  - Pros: simple, fast, scalable.
  - Cons: sensitive to blocking
  - When to use: When speed and scalability are important and
    objects do not need to be protected while blocking.

## Contributing

Contributions are welcome! Please ensure you only submit code you wrote, or you have permission to share.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in `Atom Box`, shall be licensed as MIT, without any additional terms or conditions.

Please feel free to log issues and suggest changes via pull requests.

Before raising a pull request, please ensure you have ran the following commands.

```bash
cargo fmt
cargo clippy
cargo test
```

Additionally, if any of your changes introduce new atomic loads or unsafe code, please ensure you run the Loom and Miri tests (see below).

### Running Loom tests

Atom Box is designed for use in concurrent code where threads can interleave in numerous different ways, this can be notoriously difficult to test all the different interleavings.
However, there is an excellent rust tool [Loom](https://github.com/tokio-rs/loom) created specifically for testing concurrent code.
Following any contribution please ensure you run the loom tests.
Additionally, feel free to contribute additional Loom tests.
To run the current Loom test suite run

```bash
RUSTFLAGS="--cfg loom" cargo test --test concurrency_tests --release
```

### Running Miri

To verify any changes to the unsafe code, please ensure you run [Miri](https://github.com/rust-lang/miri).
This is a verification tool for checking the validity of unsafe code.
You will need to switch to the nightly tool-chain.
You will then need to install Miri using the instructions in the repo.

To verify your code, please run the following command:

```bash
RUST_BACKTRACE=1 MIRIFLAGS="-Zmiri-ignore-leaks -Zmiri-disable-isolation" cargo +nightly miri test
```

## Code of conduct

We follow the [Rust code of conduct](https://www.rust-lang.org/policies/code-of-conduct).

Currently the moderation team consists of John Bell only. We would welcome more members: if you would like to join the moderation team, please contact John Bell.

## Licence

The project is licensed under the [MIT license](https://github.com/Johnabell/atom_box/blob/master/LICENSE).

## References

- [Lock-Free Data Structures with Hazard Pointers](https://erdani.org/publications/cuj-2004-12.pdf)
- Facebook's Folly library for C++ contains a hazard pointer implementation [Folly](https://github.com/facebook/folly)
