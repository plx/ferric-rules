(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)))
(defrule probe =>
 (delayed-do-for-all-facts ((?a item) (?b item)) (eq ?a ?b)
  (modify ?a (value (+ ?a:value 1)))
  (printout t ?a:value ":" ?b:value ":" (fact-existp ?a) ":" (fact-existp ?b) crlf))
 (printout t "live:")
 (do-for-all-facts ((?f item)) TRUE (printout t ?f:value ":"))
 (printout t crlf))
