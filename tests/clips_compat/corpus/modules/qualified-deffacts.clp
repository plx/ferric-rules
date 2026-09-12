;; A qualified deffacts belongs to its named module.
;; Level: interaction
;; Covers: modules, qualified-deffacts
(defmodule DATA (export deftemplate reading))
(deftemplate reading (slot value))
(defmodule MAIN (import DATA deftemplate reading))
(deffacts DATA::seed (reading (value 21)))
(defrule MAIN::probe (reading (value ?v)) => (printout t ?v crlf))
