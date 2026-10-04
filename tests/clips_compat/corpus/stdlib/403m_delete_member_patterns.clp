(defrule run =>
 (printout t (delete-member$ (create$ a b a c b) a b) ":"
   (delete-member$ (create$ a b a b c) (create$ a b)) ":"
   (delete-member$ (create$ a b c) b (create$ a c)) crlf))
