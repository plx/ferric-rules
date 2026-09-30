(deftemplate item (slot value))
(deffacts seed (item (value 10)))
(deffunction plus-one (?x) (bind ?x (+ ?x 1)) ?x)
(defrule probe =>
  (printout t (any-factp ((?f item)) (= (plus-one ?f:value) 11)) crlf))
