(defrule run =>
 (printout t (delete-member$ (create$ 1 1.0 0.0 -0.0 a "a") 1 0.0 a) ":"
   (replace-member$ (create$ a "a" a) X a) crlf))
