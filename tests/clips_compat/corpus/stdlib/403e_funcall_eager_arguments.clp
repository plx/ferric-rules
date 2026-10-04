(deffunction mark (?tag ?value) (printout t ?tag) ?value)
(defrule run =>
  (printout t (funcall eq a b (mark E a)) ":"
    (funcall and FALSE (mark A TRUE)) crlf))
