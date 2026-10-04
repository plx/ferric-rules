(defrule run =>
 (printout t (delete-member$ (create$ a b a c) a) " " (+ (expand$ (create$ 1 2 3))) crlf))
