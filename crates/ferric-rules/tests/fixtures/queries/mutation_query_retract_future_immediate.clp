(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defrule probe =>
  (do-for-all-facts ((?f item)) TRUE
    (printout t ?f:value crlf)
    (if (= ?f:value 10) then
      (do-for-fact ((?later item)) (= ?later:value 20) (retract ?later)))))
