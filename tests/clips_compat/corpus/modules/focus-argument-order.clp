;; The first argument to a multi-module focus runs first.
;; Level: interaction
;; Covers: modules, focus-argument-order
(defmodule FIRST)
(defrule FIRST::work => (printout t "first" crlf))
(defmodule SECOND)
(defrule SECOND::work => (printout t "second" crlf))
(defrule MAIN::start => (focus FIRST SECOND))
