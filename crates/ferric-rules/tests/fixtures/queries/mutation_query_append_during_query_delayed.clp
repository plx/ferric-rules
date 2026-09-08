(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)))
(defrule probe =>
  (delayed-do-for-all-facts ((?f item)) TRUE
    (printout t ?f:value crlf)
    (if (= ?f:value 10) then (assert (item (value 30))))))
