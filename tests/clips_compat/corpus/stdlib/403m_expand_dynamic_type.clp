(deffunction value () 3)
(defrule run => (printout t prefix (+ 1 (expand$ (value))) after))
