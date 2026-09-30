;; Issue #325 original single-binding acceptance case.
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defglobal ?*count* = 0)
(defrule probe =>
  (do-for-all-facts ((?f item)) (> ?f:value 10)
    (bind ?*count* (+ ?*count* 1)))
  (printout t ?*count* crlf))
