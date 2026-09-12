;; A module-qualified deffunction call resolves an exported function.
;; Level: interaction
;; Covers: modules, qualified-function-call
(defmodule UTIL (export deffunction ?ALL))
(deffunction twice (?x) (* ?x 2))
(defmodule MAIN (import UTIL deffunction ?ALL))
(defrule probe => (printout t (UTIL::twice 8) crlf))
