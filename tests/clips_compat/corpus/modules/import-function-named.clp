;; A named exported deffunction is callable by an importing module.
;; Level: interaction
;; Covers: modules, import-function-named
(defmodule UTIL (export deffunction twice))
(deffunction twice (?x) (* ?x 2))
(defmodule MAIN (import UTIL deffunction twice))
(defrule probe => (printout t (twice 7) crlf))
