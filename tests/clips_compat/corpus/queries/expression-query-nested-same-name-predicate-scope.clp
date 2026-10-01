
(deftemplate outer-item (slot value))
(deftemplate inner-item (slot padding) (slot value))
(deffacts seed (outer-item (value 10)) (inner-item (padding x) (value 20)))
(defrule probe =>
  (printout t
    (any-factp ((?f outer-item))
      (and (any-factp ((?f inner-item)) (= ?f:value 20)) (= ?f:value 10))) crlf)
  (do-for-all-facts ((?f outer-item)) TRUE
    (printout t (any-factp ((?f inner-item)) (= ?f:value 20)) ":" ?f:value crlf)))
