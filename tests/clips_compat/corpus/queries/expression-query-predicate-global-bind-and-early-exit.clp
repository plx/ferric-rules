
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defglobal ?*count* = 0)
(defrule probe =>
  (printout t (any-factp ((?f item)) (bind ?*count* (+ ?*count* 1))) ":" ?*count* crlf)
  (printout t (length$ (find-fact ((?f item)) (bind ?*count* (+ ?*count* 1)))) ":" ?*count* crlf)
  (printout t (length$ (find-all-facts ((?f item)) (bind ?*count* (+ ?*count* 1)))) ":" ?*count* crlf)
  (printout t "skipped:"
    (and FALSE (any-factp ((?f item)) (bind ?*count* (+ ?*count* 1)))) ":"
    (or TRUE (any-factp ((?f item)) (bind ?*count* (+ ?*count* 1)))) ":" ?*count* crlf))
