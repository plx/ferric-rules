;; CLIPS query declarations reject qualified names even when imported.
(defmodule DATA (export deftemplate item))
(deftemplate DATA::item (slot value))
(deffacts DATA::seed (item (value 10)))
(defmodule MAIN (import DATA deftemplate item))
(defrule MAIN::probe => (printout t (any-factp ((?f DATA::item)) TRUE) crlf))
