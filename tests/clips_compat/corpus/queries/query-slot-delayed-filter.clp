(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defrule probe =>
  (bind ?count 0)
  (delayed-do-for-all-facts ((?f item)) (> ?f:value 10)
    (bind ?count (+ ?count 1)))
  (printout t ?count crlf))
