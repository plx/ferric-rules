; Cross-module import: UTILS exports a function; MAIN imports and calls it.
(defmodule UTILS (export deffunction ?ALL))
(deffunction square (?x) (* ?x ?x))

(defmodule MAIN (import UTILS deffunction ?ALL))

(deffacts MAIN::startup (compute))

(defrule MAIN::run-compute
    (compute)
    =>
    (printout t "square-5: " (square 5) crlf)
    (printout t "square-9: " (square 9) crlf))
