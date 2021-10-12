# ptr-hazard
A safe implementation of hazard pointers in rust

The aim of this project is to provide a safe and idomatic rust API for using hazard pointers for safe memory reclamation in multi-threaded concurrent data structures.

## References
 - [Lock-Free Data Structures with Hazard Pointers](https://erdani.org/publications/cuj-2004-12.pdf)
 - Facebooks Folly library for C++ contains a hazard pointer implementation [Folly](https://github.com/facebook/folly)
