(deftemplate item (slot value))
(deffacts seed (item (value 10)))
(defrule probe =>
  (do-for-all-facts ((?f item)) TRUE
    (if TRUE then (bind ?f 42))))
