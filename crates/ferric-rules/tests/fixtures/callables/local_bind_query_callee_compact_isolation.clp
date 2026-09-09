(deftemplate item (slot value))
(deffacts seed (item (value 10)))
(deffunction local-slot (?x) (bind ?f:value ?x) ?f:value)
(defrule probe =>
  (printout t (any-factp ((?f item)) (= (local-slot 91) 91)) ":" (local-slot 92) crlf))
