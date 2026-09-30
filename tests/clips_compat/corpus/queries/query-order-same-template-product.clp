(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)))
(defrule probe =>
 (do-for-all-facts ((?a item) (?b item)) TRUE
   (printout t (fact-slot-value ?a value) ":" (fact-slot-value ?b value) crlf)))
