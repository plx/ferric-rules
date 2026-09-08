(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (marker))
(defrule probe => (do-for-fact ((?f item)) TRUE (retract ?f) (retract ?f) (printout t "after" crlf)))
