;; A named exported global is imported and mutated unqualified.
;; Level: interaction
;; Covers: modules, import-global-named
(defmodule CONFIG (export defglobal threshold))
(defglobal ?*threshold* = 10)
(defmodule MAIN (import CONFIG defglobal threshold))
(defrule probe =>
    (bind ?*threshold* (+ ?*threshold* 1))
    (printout t ?*threshold* crlf))
