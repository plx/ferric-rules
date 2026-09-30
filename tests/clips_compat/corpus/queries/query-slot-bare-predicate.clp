(deftemplate item (slot value) (slot enabled))
(deffacts seed
  (item (value 10) (enabled FALSE)) (item (value 20) (enabled TRUE)))
(defrule probe =>
  (bind ?sum 0)
  (do-for-all-facts ((?f item)) ?f:enabled
    (if ?f:enabled then (bind ?sum (+ ?sum ?f:value))))
  (printout t ?sum crlf))
