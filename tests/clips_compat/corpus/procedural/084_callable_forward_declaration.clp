(deffunction later (?x))
(deffunction earlier (?x) (later ?x))
(deffunction later (?x) (+ ?x 1))
(defrule run => (printout t (earlier 1) crlf))
