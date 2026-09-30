// Package ferric provides Go bindings for the ferric rules engine,
// a high-performance CLIPS-compatible production rule system.
//
// Create an Engine with NewEngine and Close it when done. Engine methods
// serialize native access internally, so one Engine may be used from several
// goroutines; use one Engine per goroutine for parallel work.
package ferric
