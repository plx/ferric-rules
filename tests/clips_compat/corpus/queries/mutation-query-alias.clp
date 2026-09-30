(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (marker))
(defrule probe => (do-for-fact ((?f item)) TRUE (bind ?saved ?f) (retract ?saved)) (printout t (length$ (find-all-facts ((?g item)) TRUE)) crlf))
