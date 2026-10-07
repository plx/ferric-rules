(deffunction mark (?tag ?value) (printout t ?tag) ?value)
(defrule run =>
  (printout t "prefix:" (funcall abs (mark A 1) (mark B 2)) "after" crlf))
