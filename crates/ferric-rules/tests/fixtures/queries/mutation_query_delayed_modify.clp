(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (marker))
(defglobal ?*sum* = 0)
(defrule probe => (delayed-do-for-all-facts ((?f item)) TRUE (modify ?f (value (+ ?f:value 1)))) (do-for-all-facts ((?f item)) TRUE (bind ?*sum* (+ ?*sum* ?f:value))) (printout t ?*sum* crlf))
