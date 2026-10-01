(deftemplate item (slot value))
(deffacts seed (item (value 10)))
(defrule probe =>
  (do-for-all-facts ((?f item)) TRUE
    (printout t ?f:missing crlf)))
