// Package ferric provides Go bindings for the ferric rules engine,
// a high-performance CLIPS-compatible production rule system.
//
// For simple, single-engine use, create an Engine directly with NewEngine.
// For concurrent, multi-engine-type use, create a Coordinator with
// NewCoordinator and obtain Manager handles for each engine type.
package ferric
