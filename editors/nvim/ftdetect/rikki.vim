au BufRead,BufNewFile *.rk setfiletype rikki
" tk shebang scripts have no extension
au BufRead,BufNewFile * if getline(1) =~# '^#!.*\<tk\>$' | setfiletype rikki | endif
