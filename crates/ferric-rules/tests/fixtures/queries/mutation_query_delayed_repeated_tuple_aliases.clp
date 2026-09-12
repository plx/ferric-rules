(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)))
(defrule probe =>
 (delayed-do-for-all-facts ((?a item) (?b item)) TRUE
  (printout t ?a:value ":" ?b:value ":" (fact-existp ?a) ":" (fact-existp ?b) crlf)
  (retract ?a ?b ?a))
 (printout t "remaining:" (length$ (find-all-facts ((?f item)) TRUE)) crlf))
