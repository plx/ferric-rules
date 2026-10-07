(defrule run =>
 (printout t (replace-member$ (create$ a b c a b) X b (create$ a b)) ":"
   (replace-member$ (create$ a b c) X a (create$ a b)) ":"
   (replace-member$ (create$ a b c) X (create$ a b) a) crlf))
