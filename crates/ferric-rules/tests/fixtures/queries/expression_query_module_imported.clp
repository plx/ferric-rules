(defmodule DATA (export deftemplate item))
(deftemplate DATA::item (slot value))
(deffacts DATA::seed (item (value 10)))
(defmodule MAIN (import DATA deftemplate item))
(defrule MAIN::probe =>
  (printout t (any-factp ((?f item)) (= ?f:value 10)) ":"
    (length$ (find-all-facts ((?f item)) TRUE)) crlf))
