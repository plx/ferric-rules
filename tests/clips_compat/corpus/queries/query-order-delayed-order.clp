(deftemplate item (slot value))
(deffacts seed (item (value 30)) (item (value 10)) (item (value 20)))
(defrule probe =>
 (delayed-do-for-all-facts ((?f item)) TRUE
   (printout t (fact-slot-value ?f value) crlf)))
