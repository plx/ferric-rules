;; An all-construct import exposes both a template and a function.
;; Level: interaction
;; Covers: modules, import-all-constructs
(defmodule DATA (export ?ALL))
(deftemplate reading (slot value))
(deffacts seed (reading (value 9)))
(deffunction twice (?x) (* ?x 2))
(defmodule MAIN (import DATA ?ALL))
(defrule probe (reading (value ?x)) => (printout t (twice ?x) crlf))
