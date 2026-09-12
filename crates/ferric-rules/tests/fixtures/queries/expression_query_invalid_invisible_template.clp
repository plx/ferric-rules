(defmodule DATA)
(deftemplate DATA::item (slot value))
(defmodule MAIN)
(defrule MAIN::probe => (printout t (any-factp ((?f item)) TRUE) crlf))
