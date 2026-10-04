(deftemplate p (slot x (default 4)))
(defrule run =>
  (printout t (fact-slot-value (assert-string "(p)") x) ":"
    (fact-slot-value (str-assert "(p (x (+ 3 4))) (p (x 9))") x) ":"
    (fact-slot-value (assert-string "(q (+ 1 2)) \"unfinished") implied) crlf))
