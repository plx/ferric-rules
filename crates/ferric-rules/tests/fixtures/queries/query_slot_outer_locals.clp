(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defrule probe =>
  (bind ?f 42)
  (bind ?threshold 10)
  (bind ?sum 0)
  (do-for-all-facts ((?f item)) (> ?f:value ?threshold)
    (bind ?sum (+ ?sum ?f:value)))
  (printout t ?sum ":" ?threshold ":" ?f crlf))
