(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defglobal ?*limit* = 100)
(defrule probe =>
  (do-for-all-facts ((?f item)) (< ?f:value ?*limit*)
    (printout t ?f:value crlf)
    (bind ?*limit* 0)))
