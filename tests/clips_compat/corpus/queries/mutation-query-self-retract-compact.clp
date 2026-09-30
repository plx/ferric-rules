(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (marker))
(defrule probe => (do-for-fact ((?f item)) TRUE (retract ?f) (printout t "exists:" (fact-existp ?f) ":value:" ?f:value crlf)))
