; MODULE::name syntax: qualified function call and qualified global read.
; Demonstrates that MATH::add resolves to the function defined in module MATH,
; and ?*CONFIG::threshold* resolves to the global defined in module CONFIG.

(defmodule MATH (export deffunction ?ALL))
(deffunction add (?x ?y) (+ ?x ?y))

(defmodule CONFIG (export defglobal ?ALL))
(defglobal ?*threshold* = 10)

(defmodule MAIN
    (import MATH deffunction ?ALL)
    (import CONFIG defglobal ?ALL))

(deffacts MAIN::startup (run-test))

(defrule MAIN::test-qualified-call
    (run-test)
    =>
    (printout t "sum: " (MATH::add 3 4) crlf)
    (printout t "threshold: " ?*CONFIG::threshold* crlf))
