; Module-qualified global: ?*MODULE::name* syntax for read and write.
; CONFIG exports its global; MAIN imports and binds it via fully-qualified name.
(defmodule CONFIG (export defglobal ?ALL))
(defglobal ?*base-value* = 10)

(defmodule MAIN (import CONFIG defglobal ?ALL))

(deffacts MAIN::startup (run-it))

(defrule MAIN::update-and-read
    (run-it)
    =>
    (bind ?*CONFIG::base-value* (* ?*CONFIG::base-value* 3))
    (printout t "value: " ?*CONFIG::base-value* crlf))
