;; An imported template matches facts asserted in its defining module.
;; Level: interaction
;; Covers: modules, import-template-named
(defmodule DATA (export deftemplate reading))
(deftemplate reading (slot value))
(deffacts seed (reading (value 17)))
(defmodule MAIN (import DATA deftemplate reading))
(defrule probe (reading (value ?x)) => (printout t ?x crlf))
