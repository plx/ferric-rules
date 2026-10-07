(defrule test => (printout t (eq a b (/ 1 0)) "|" (neq a a (/ 1 0)) "|" (eq (create$ a b) (create$ a b) (create$ a b)) crlf))
