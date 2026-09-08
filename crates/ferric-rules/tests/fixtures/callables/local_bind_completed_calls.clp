(deffunction calculate (?x) (bind ?result (+ ?x 1)) (* ?result 2))
(defgeneric transform)
(defmethod transform ((?x NUMBER)) (bind ?local (+ ?x 1)) (bind ?x 88) ?local)
(defmethod transform ((?x INTEGER))
  (bind ?local (+ ?x 10)) (bind ?x 99)
  (bind ?lower (call-next-method)) (create$ ?local ?x ?lower))
(defrule first-call => (printout t "first:" (calculate 3) ":" (transform 3) crlf))
;; CALL AFTER COMPLETION
(defrule next-call => (printout t "next:" (calculate 5) ":" (transform 5) crlf))
