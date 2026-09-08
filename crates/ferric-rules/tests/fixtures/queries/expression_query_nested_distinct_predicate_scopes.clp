
(deftemplate outer-item (slot value))
(deftemplate inner-item (slot padding) (slot value))
(deffacts seed
  (outer-item (value 10)) (outer-item (value 30))
  (inner-item (padding x) (value 20)))
(defrule probe =>
  (bind ?found (find-all-facts ((?f outer-item))
    (and (any-factp ((?g inner-item)) (> ?g:value ?f:value)) (= ?f:value 10))))
  (printout t (length$ ?found) ":" (fact-slot-value (nth$ 1 ?found) value) crlf)
  (do-for-all-facts ((?f outer-item)) (= ?f:value 10)
    (printout t (any-factp ((?g inner-item)) (> ?g:value ?f:value)) ":" ?f:value crlf)))
