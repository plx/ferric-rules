;; Distinct and reused query names resolve in their lexical query scope.
(deftemplate outer-item (slot value))
(deftemplate inner-item (slot padding) (slot value))
(deffacts seed
  (outer-item (value 10))
  (inner-item (padding a) (value 20)) (inner-item (padding b) (value 30)))
(defglobal ?*sum* = 0)
(defrule probe =>
  (do-for-all-facts ((?f outer-item)) TRUE
    (do-for-all-facts ((?g inner-item)) (> ?g:value ?f:value)
      (bind ?*sum* (+ ?*sum* ?f:value ?g:value)))
    (printout t "outer:" ?f:value crlf)
    (do-for-all-facts ((?f inner-item)) (= ?f:value 20)
      (printout t "inner:" ?f:value crlf))
    (printout t "restored:" ?f:value crlf))
  (printout t "sum:" ?*sum* crlf))
