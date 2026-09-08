(deftemplate item (slot value))
(deffacts seed (item (value 30)) (item (value 10)) (item (value 20)))
(defrule probe =>
 (do-for-fact ((?f item)) (< (fact-slot-value ?f value) 30)
   (printout t (fact-slot-value ?f value) crlf)))
