(defrule run =>
 (printout t (replace-member$ (create$ a b a c) (create$) a) ":"
   (replace-member$ (create$ a b c) (create$) b (create$ a c)) ":"
   (replace-member$ (create$ a a) (create$ a b) a) crlf))
