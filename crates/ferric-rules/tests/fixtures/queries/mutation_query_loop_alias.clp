(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)))
(defrule probe =>
  (do-for-fact ((?f item)) (= ?f:value 10)
    (bind ?saved ?f)
    (loop-for-count (?f 1 1)
      (retract ?saved)
      (printout t ?f ":" ?f:value ":" (fact-existp ?saved) crlf)))
  (printout t "remaining:" (length$ (find-all-facts ((?g item)) TRUE)) crlf))
